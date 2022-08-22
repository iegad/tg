use super::{pack, IProc, Server, CONN_POOL};
use crate::g;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::{
    net::{TcpListener, TcpStream},
    select, signal,
    sync::broadcast,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

pub async fn run<TProc>(server: &Server, proc: TProc)
where
    TProc: IProc,
{
    if let Err(err) = proc.on_init(server) {
        println!("proc.on_init failed: {:?}", err);
        return;
    }

    let listener = match TcpListener::bind(server.host).await {
        Ok(l) => l,
        Err(err) => panic!("ws server bind failed: {:?}", err),
    };

    let (notify_shutdown, mut shutdown) = broadcast::channel(1);

    'server_loop: loop {
        let permit = server
            .limit_connections
            .clone()
            .acquire_owned()
            .await
            .unwrap();

        select! {
            res_accept = listener.accept() => {
                let (stream, addr) = match res_accept {
                    Ok((s, addr)) => (s, addr),
                    Err(err) => {
                        println!("accept failed: {:?}", err);
                        break 'server_loop;
                    }
                };

                let shutdown = notify_shutdown.subscribe();
                tokio::spawn(async move{
                    conn_handle(stream, addr, proc, shutdown).await;
                    drop(permit);
                });
            },

            _ = signal::ctrl_c() => {
                if let Err(err) = notify_shutdown.send(1) {
                    panic!("shutdown_sender.send failed: {:?}", err);
                }
            }

            _ = shutdown.recv() => {
                break 'server_loop;
            }
        }
    }
}

async fn conn_handle<TProc>(
    stream: TcpStream,
    _addr: SocketAddr,
    proc: TProc,
    mut notify_shutdown: broadcast::Receiver<u8>,
) where
    TProc: IProc,
{
    let mut conn = CONN_POOL.pull();
    if let Err(err) = conn.init(&stream) {
        println!("conn.init failed: {:?}", err);
        return;
    }

    let ws_stream = match accept_async(stream).await {
        Ok(s) => s,
        Err(err) => {
            println!("conn upgrade to websocket failed: [{:?}]", err);
            return;
        }
    };

    if let Err(err) = proc.on_connected(&*conn) {
        println!("proc.on_connected failed: {:?}", err);
        return;
    }

    let (mut writer, mut reader) = ws_stream.split();
    let (wch_sender, mut wch_receiver) = tokio::sync::mpsc::channel(g::DEFAULT_CHAN_SIZE);
    let mut timeout_ticker =
        tokio::time::interval(std::time::Duration::from_secs(g::DEFAULT_READ_TIMEOUT));

    'conn_loop: loop {
        select! {
            _ = notify_shutdown.recv() => {
                println!("server is shutdown");
                break 'conn_loop;
            }

            _ = timeout_ticker.tick() => {
                proc.on_conn_error(&*conn, g::Err::TcpReadTimeout);
                break 'conn_loop;
            }

            result_rsp = wch_receiver.recv() => {
                let mut rsp: pack::PackageItem = match result_rsp {
                    None => panic!("failed wch rsp"),
                    Some(v) => v,
                };

                if let Err(err) = writer.send(Message::Binary(rsp.to_bytes().to_vec())).await {
                    proc.on_conn_error(&*conn, g::Err::TcpWriteFailed(format!("wsWriteFailed: {:?}", err)));
                    break 'conn_loop;
                }
            }

            result_read = reader.next() => {
                let msg = match result_read {
                    Some(result_msg) => {
                        match result_msg {
                            Err(err) => {
                                proc.on_conn_error(&*conn, g::Err::TcpReadFailed(format!("wsReadFailed: {:?}", err)));
                                break 'conn_loop;
                            }

                            Ok(msg) => msg,
                        }
                    }
                    None => {
                        proc.on_conn_error(&*conn, g::Err::TcpReadFailed("wsReadFailed: none".to_string()));
                        break 'conn_loop;
                    }
                };

                match msg {
                    Message::Binary(v) => {
                        conn.req_mut().as_mut_bytes().copy_from_slice(&v);
                        let ok = match conn.req_mut().parse(v.len()) {
                            Err(err) => {
                                proc.on_conn_error(&*conn, g::Err::TcpReadFailed(format!("wsReadFailed: {:?}", err)));
                                break 'conn_loop;
                            }

                            Ok(v) => v,
                        };

                        if ok {
                            conn.recv_seq_incr();
                            let rsp = match proc.on_process(&*conn) {
                                Err(_) => break 'conn_loop,
                                Ok(rsp) => rsp,
                            };

                            if let Err(err) = wch_sender.send(rsp).await {
                                panic!("write channel[mpsc] send failed: {}", err);
                            }
                        }
                    }

                    Message::Ping(_) => {}
                    Message::Pong(_) => {}

                    Message::Text(_) => {
                        proc.on_conn_error(&*conn, g::Err::TcpReadFailed("wsReadFailed: text".to_string()));
                        break 'conn_loop;
                    }

                    Message::Close(_) => {
                        break 'conn_loop;
                    }

                    Message::Frame(_) => {}
                }
            }
        }
    }
}

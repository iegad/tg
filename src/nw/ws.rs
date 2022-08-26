use super::{conn::CONN_POOL, pack, IProc, Server};
use crate::g;
use futures_util::{SinkExt, StreamExt};
use std::{net::SocketAddr, time::Duration};
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
    proc.on_init(server).await;

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

    proc.on_released(server).await;
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
    conn.from_with(&stream);

    let ws_stream = match accept_async(stream).await {
        Ok(s) => s,
        Err(err) => {
            println!("conn upgrade to websocket failed: [{:?}]", err);
            return;
        }
    };

    if let Err(err) = proc.on_connected(&*conn).await {
        println!("proc.on_connected failed: {:?}", err);
        return;
    }

    let (mut writer, mut reader) = ws_stream.split();
    let mut timeout_ticker = tokio::time::interval(Duration::from_secs(g::DEFAULT_READ_TIMEOUT));
    let tx = conn.sender();
    let mut rx = conn.receiver();
    let mut req = pack::PACK_POOL.pull();

    'conn_loop: loop {
        select! {
            _ = notify_shutdown.recv() => {
                println!("server is shutdown");
                break 'conn_loop;
            }

            _ = timeout_ticker.tick() => {
                proc.on_conn_error(&*conn, g::Err::TcpReadTimeout).await;
                break 'conn_loop;
            }

            result_rsp = rx.recv() => {
                let rsp = match result_rsp {
                    Err(_) => panic!("failed wch rsp"),
                    Ok(v) => v,
                };

                if let Err(err) = writer.send(Message::Binary(rsp.to_vec())).await {
                    proc.on_conn_error(&*conn, g::Err::TcpWriteFailed(format!("wsWriteFailed: {:?}", err))).await;
                    break 'conn_loop;
                }
            }

            result_read = reader.next() => {
                let msg = match result_read {
                    Some(result_msg) => {
                        match result_msg {
                            Err(err) => {
                                proc.on_conn_error(&*conn, g::Err::TcpReadFailed(format!("wsReadFailed: {:?}", err))).await;
                                break 'conn_loop;
                            }

                            Ok(msg) => msg,
                        }
                    }
                    None => {
                        proc.on_conn_error(&*conn, g::Err::TcpReadFailed("wsReadFailed: none".to_string())).await;
                        break 'conn_loop;
                    }
                };

                match msg {
                    Message::Binary(v) => {
                        let ok = match req.parse(&v) {
                            Err(err) => {
                                proc.on_conn_error(&*conn, g::Err::TcpReadFailed(format!("wsReadFailed: {:?}", err))).await;
                                break 'conn_loop;
                            }

                            Ok(v) => v,
                        };

                        if ok {
                            conn.recv_seq_incr();
                            let rsp = match proc.on_process(&conn, &req).await {
                                Err(_) => break 'conn_loop,
                                Ok(rsp) => rsp,
                            };

                            if let Err(err) = tx.send(rsp) {
                                panic!("write channel[mpsc] send failed: {}", err);
                            }
                        }
                    }

                    Message::Ping(_) => {}
                    Message::Pong(_) => {}

                    Message::Text(_) => {
                        proc.on_conn_error(&*conn, g::Err::TcpReadFailed("wsReadFailed: text".to_string())).await;
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

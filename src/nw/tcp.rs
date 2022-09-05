use super::{Conn, IEvent, IServerEvent, Server};
use crate::g;
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select, signal,
    sync::broadcast,
};

pub async fn server_run<T: IServerEvent>(server: &Server<T>) -> io::Result<()> {
    let listener = TcpListener::bind(server.host()).await?;
    let (notfiy_shutdown, mut shutdown) = broadcast::channel(1);

    server
        .event
        .on_runing(server.host, server.max_connections, server.timeout)
        .await;

    loop {
        select! {
            result_accept =  listener.accept() => {
                let (stream, _) =  result_accept?;
                let permit = server.limit_connections.clone().acquire_owned().await.unwrap();
                let event = server.event.clone();
                let shutdown = notfiy_shutdown.subscribe();
                let timeout = server.timeout;

                tokio::spawn(async move {
                    conn_handle(stream, timeout, shutdown, event).await;
                    drop(permit);
                });
            }

            _ = signal::ctrl_c() => {
                if let Err(err) = notfiy_shutdown.send(1) {
                    panic!("notfiy_shutdown send failed: {:?}", err);
                }
            }

            _ = shutdown.recv() => {
                break;
            }
        }
    }

    server.event.on_stopped(server.host).await;
    Ok(())
}

pub async fn conn_handle<T: IEvent>(
    mut stream: TcpStream,
    timeout: u64,
    mut shutdown: broadcast::Receiver<u8>,
    event: T,
) {
    let mut conn = Conn::new();
    conn.load_from(&stream);

    let (mut reader, mut writer) = stream.split();
    let tx = conn.sender();
    let mut rx = conn.receiver();
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(timeout));

    if let Err(_) = event.on_connected(&conn).await {
        return;
    }

    loop {
        ticker.reset();

        select! {
            _ = shutdown.recv() => {
                break;
            }

            _ = ticker.tick() => {
                event.on_error(&conn, g::Err::TcpReadTimeout).await;
                break;
            }

            result_wbuf = rx.recv() => {
                let wbuf = match result_wbuf {
                    Err(err) => panic!("wch recv failed: {:?}", err),
                    Ok(v) => v,
                };

                if let Err(err) = writer.write_all(&wbuf).await {
                    event.on_error(&conn, g::Err::TcpWriteFailed(format!("{:?}", err))).await;
                    break;
                }
                conn.send_seq += 1;
            }

            result_read = reader.read_buf(conn.rbuf_mut()) => {
                match result_read {
                    Err(err) => {
                        event.on_error(&conn, g::Err::TcpReadFailed(format!("{:?}", err))).await;
                        break;
                    }
                    Ok(0) => break,
                    Ok(n) => println!("{}", hex::encode(&conn.rbuf()[..n])),
                }

                conn.recv_seq += 1;

                let option_rsp = match event.on_process(&conn, &conn.rbuf()).await {
                    Err(_) => break,
                    Ok(v) => v,
                };

                if let Some(rsp) = option_rsp {
                    tx.send(rsp).unwrap();
                }

                conn.rbuf_mut().clear();
            }
        }
    }

    event.on_disconnected(&conn).await;
}

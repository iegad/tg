use super::{pack, IEvent, IServerEvent, Server};
use crate::{g, us::Ptr};
use lockfree_object_pool::LinearObjectPool;
use std::sync::{atomic::Ordering, Arc};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream},
    select,
    sync::broadcast,
};

pub async fn server_run<T>(
    server: Arc<Ptr<Server<T>>>,
    conn_pool: &'static LinearObjectPool<super::Conn<T::U>>,
) -> io::Result<()>
where
    T: IServerEvent,
    T::U: Default + Sync + Send,
{
    let lfd = TcpSocket::new_v4()?;

    #[cfg(unix)]
    lfd.set_reuseport(true)?;
    lfd.set_reuseaddr(true)?;
    lfd.bind(server.host().parse().unwrap())?;

    let listener = lfd.listen(1024)?;
    let notify_shutdown = server.shutdown.clone();
    let mut shutdown = server.shutdown.subscribe();

    server.event.on_runing(server.clone()).await;
    server.running.store(true, Ordering::SeqCst);

    'accept_loop: loop {
        select! {
            _ = shutdown.recv() => {
                tracing::debug!("accept_loop is breaking...");
                break 'accept_loop;
            }

            result_accept = listener.accept() => {
                let (stream, _) =  result_accept?;
                let permit = server.limit_connections.clone().acquire_owned().await.unwrap();
                let event = server.event.clone();
                let shutdown = notify_shutdown.subscribe();
                let timeout = server.timeout;
                tokio::spawn(async move {
                    conn_handle(stream, conn_pool, timeout, shutdown, event, permit).await;
                });
            }
        }
    }

    server.get_mut().running.store(false, Ordering::SeqCst);
    server.event.on_stopped(server.clone()).await;

    Ok(())
}

pub async fn conn_handle<T>(
    mut stream: TcpStream,
    conn_pool: &'static LinearObjectPool<super::Conn<T::U>>,
    timeout: u64,
    mut shutdown: broadcast::Receiver<u8>,
    event: T,
    permit: tokio::sync::OwnedSemaphorePermit,
) where
    T: IEvent,
{
    let mut conn = conn_pool.pull();
    conn.load_from(&stream);

    let (mut reader, mut writer) = stream.split();
    let w_tx = conn.wbuf_sender();
    let mut w_rx = conn.wbuf_receiver();
    let mut shutdown_rx = conn.shutdown_receiver();
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(timeout));
    let mut req = pack::PACK_POOL.pull();

    if let Err(_) = event.on_connected(&conn).await {
        return;
    }

    'conn_loop: loop {
        ticker.reset();

        select! {
            _ = shutdown.recv() => {
                conn.shutdown();
            }

            _ = shutdown_rx.recv() => {
                tracing::debug!("[{:05}-{:?}] conn_loop is breaking...", conn.sockfd, conn.remote);
                break 'conn_loop;
            }

            _ = ticker.tick() => {
                event.on_error(&conn, g::Err::TcpReadTimeout).await;
                break 'conn_loop;
            }

            result_wbuf = w_rx.recv() => {
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
                        break 'conn_loop;
                    }
                    Ok(0) => break 'conn_loop,
                    Ok(_) => (),
                }

                loop {
                    if req.valid() {
                        conn.recv_seq += 1;
                        let option_rsp = match event.on_process(&conn, &req).await {
                            Err(_) => break 'conn_loop,
                            Ok(v) => v,
                        };

                        req.reset();
                        if let Some(rsp_bytes) = option_rsp {
                            if let Err(_) = w_tx.send(rsp_bytes) {
                                tracing::error!("w_tx.send failed");
                            }
                        }
                    } else {
                        if let Err(err) = pack::Package::parse(conn.rbuf_mut(), &mut req) {
                            event.on_error(&conn, err).await;
                            break 'conn_loop;
                        }
                    }

                    if !req.valid() && conn.rbuf_mut().len() < pack::Package::HEAD_SIZE {
                        break;
                    }
                }

                conn.check_rbuf();
            }
        }
    }

    drop(permit);
    event.on_disconnected(&conn).await;
}

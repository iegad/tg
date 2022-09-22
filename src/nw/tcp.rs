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

/// # server_run<T>
///
/// run a tcp server.
///
/// # Parameters
///
/// `server` Arc<ServerPtr<T>> instance.
///
/// `conn_pool` Conn<T::U> object pool.
pub async fn server_run<T>(
    server: Arc<Ptr<Server<T>>>,
    conn_pool: &'static LinearObjectPool<super::Conn<T::U>>,
) -> io::Result<()>
where
    T: IServerEvent,
    T::U: Default + Sync + Send,
{
    // step 1: init tcp listener.
    let lfd = TcpSocket::new_v4()?;

    #[cfg(unix)]
    lfd.set_reuseport(true)?;
    lfd.set_reuseaddr(true)?;
    lfd.bind(server.host().parse().unwrap())?;
    let listener = lfd.listen(1024)?;

    // step 2: get shutdown sender and receiver
    let shutdown_tx = server.shutdown.clone();
    let mut shutdown_rx = server.shutdown.subscribe();

    // step 3: trigge server running event.
    server.event.on_running(server.clone()).await;

    // step 4: set server state running(true).
    server.running.store(true, Ordering::SeqCst);

    // step 5: accept_loop.
    'accept_loop: loop {
        select! {
            // when recv shutdown signal.
            _ = shutdown_rx.recv() => {
                tracing::debug!("accept_loop is breaking...");
                break 'accept_loop;
            }

            // when connection comming.
            result_accept = listener.accept() => {
                let (stream, _) =  result_accept?;
                let permit = server.limit_connections.clone().acquire_owned().await.unwrap();
                let event = server.event.clone();
                let shutdown = shutdown_tx.subscribe();
                let timeout = server.timeout;
                tokio::spawn(async move {
                    conn_handle(stream, conn_pool, timeout, shutdown, event, permit).await;
                });
            }
        }
    }

    // step 6: set server state running(false).
    server.get_mut().running.store(false, Ordering::SeqCst);

    // step 7: trigger server stopped event.
    server.event.on_stopped(server.clone()).await;

    Ok(())
}

/// # conn_handle<T>
///
/// tcp connection's handler.
///
/// # Parameters
///
/// `stream` [TcpStream]
///
/// `conn_pool` lockfree object pool.
///
/// `timeout` read timeout
///
/// `shutdown_rx` server shutdown signal receiver.
///
/// `event` IEvent implement.
///
/// `permit` Limit Semaphore.
pub async fn conn_handle<T>(
    mut stream: TcpStream,
    conn_pool: &'static LinearObjectPool<super::Conn<T::U>>,
    timeout: u64,
    mut shutdown_rx: broadcast::Receiver<u8>,
    event: T,
    permit: tokio::sync::OwnedSemaphorePermit,
) where
    T: IEvent,
{
    // step 1: get a default Conn<U>.
    let mut conn = conn_pool.pull();

    // step 2: active Conn<U>.
    conn.acitve(&stream);

    // step 3: get socket reader and writer
    let (mut reader, mut writer) = stream.split();

    // step 4: get write channel receiver and sender.
    let w_tx = conn.wbuf_sender();
    let mut w_rx = conn.wbuf_receiver();

    // step 5: get conn shutdown channel receiver.
    let mut conn_shutdown_rx = conn.shutdown_receiver();

    // step 6: timeout ticker.
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(timeout));

    // step 7: get request instance.
    let mut req = pack::PACK_POOL.pull();

    // step 8: triger connected event.
    if let Err(_) = event.on_connected(&conn).await {
        return;
    }

    // step 9: conn_loop.
    'conn_loop: loop {
        ticker.reset();

        select! {
            // server shutdown signal
            _ = shutdown_rx.recv() => {
                conn.shutdown();
            }

            // connection shutdown signal
            _ = conn_shutdown_rx.recv() => {
                tracing::debug!("[{:05}-{:?}] conn_loop is breaking...", conn.sockfd, conn.remote);
                break 'conn_loop;
            }

            // timeout ticker
            _ = ticker.tick() => {
                event.on_error(&conn, g::Err::TcpReadTimeout).await;
                break 'conn_loop;
            }

            // get response from write channel to write to remote.
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

            // connection read data.
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

    // step 10: returns connections permit.
    drop(permit);

    // step 11: trigger disconnected event.
    event.on_disconnected(&conn).await;
}

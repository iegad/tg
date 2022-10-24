use super::{server::Server, conn::{Pool, Ptr}};
use futures::StreamExt;
use crate::g;
use std::sync::{atomic::Ordering, Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream},
    select,
    sync::{broadcast, OwnedSemaphorePermit},
};

// ---------------------------------------------- server_run ----------------------------------------------
//
//
/// # server_run<T>
///
/// run a tcp server.
///
/// # Parameters
///
/// `server` Arc<ServerPtr<T>> instance.
///
/// `conn_pool` Conn<T::U> object pool.
pub async fn server_run<'a, T>(
    server: Arc<Server<T>>,
    conn_pool: &'static Pool<'a, T::U>,
) -> g::Result<()>
where
    T: super::server::IEvent,
    T::U: Default + Sync + Send,
{
    // step 1: 初始化监听套接字
    let lfd = match TcpSocket::new_v4() {
        Err(err) => return Err(g::Err::SocketErr(format!("{err}"))),
        Ok(v) => v,
    };

    #[cfg(unix)]
    {
        if let Err(err) = lfd.set_reuseport(true) {
            return Err(g::Err::SocketErr(format!("{err}")));
        }
    }

    if let Err(err) = lfd.set_reuseaddr(true) {
        return Err(g::Err::SocketErr(format!("{err}")));
    }

    if let Err(err) = lfd.bind(server.host().parse().unwrap()) {
        return Err(g::Err::SocketErr(format!("{err}")));
    }

    let listener = match lfd.listen(1024) {
        Err(err) => return Err(g::Err::SocketErr(format!("{err}"))),
        Ok(v) => v,
    };

    // step 2: 改变server 运行状态
    if server.running.swap(true, Ordering::SeqCst) {
        return Err(g::Err::ServerIsAlreadyRunning);
    }

    // step 3: 获取关闭句柄
    let mut shutdown_rx = server.shutdown_tx.subscribe();

    // step 4: trigge server running event.
    server.event.on_running(&server).await;

    // step 5: accept_loop.
    let timeout = server.timeout;

    'accept_loop: loop {
        let event = server.event.clone();
        let shutdown_receiver = shutdown_rx.resubscribe();
        let conn = Arc::new(conn_pool.pull());

        select! {
            // when recv shutdown signal.
            _ = shutdown_rx.recv() => {
                break 'accept_loop;
            }

            // when connection comming.
            result_accept = listener.accept() => {
                let stream =  match result_accept {
                    Err(err) => return Err(g::Err::ServerAcceptError(format!("{err}"))),
                    Ok((v, _)) => v
                };

                let permit = server.limit_connections.clone().acquire_owned().await.unwrap();
                tokio::spawn(async move {
                    conn_handle(stream, conn, timeout, shutdown_receiver, event, permit).await;
                });
            }
        }
    }

    // step 6: set server state running(false).
    server.running.store(false, Ordering::SeqCst);

    // step 7: trigger server stopped event.
    server.event.on_stopped(&server).await;

    Ok(())
}

// ---------------------------------------------- conn handle ----------------------------------------------
//
//
/// # conn_handle<T>
///
/// tcp connection's handler.
///
/// # Parameters
///
/// `stream` [TcpStream]
///
/// `conn_resuable` [LinearReusable<'static, Conn<T::U>>]
///
/// `timeout` read timeout
///
/// `shutdown_rx` server shutdown signal receiver.
///
/// `event` IEvent implement.
///
async fn conn_handle<'a, T: super::server::IEvent>(
    stream: TcpStream,
    conn: Ptr<'_, T::U>,
    timeout: u64,
    mut shutdown_rx: broadcast::Receiver<u8>,
    event: T,
    permit: OwnedSemaphorePermit,
) {
    // step 1: get socket reader and writer
    let (mut reader, writer) = stream.into_split();
    let mut srx = conn.setup(&reader);
    let srxc = srx.resubscribe();
    
    let eventc = event.clone();
    let connc = conn.clone();

    let writer_future = tokio::spawn(async move {
        conn_write_handle(connc, eventc, writer, srxc).await;
    });

    // step 3: timeout ticker.
    let interval = if timeout == 0 { 60 * 60} else { timeout };
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));

    if event.on_connected(&conn).await.is_err() {
        return;
    }

    'read_loop: loop {
        ticker.reset();

        select! {
            _ = ticker.tick() => {
                if timeout > 0 {
                    event.on_error(&conn, g::Err::IOReadTimeout).await;
                    conn.shutdown();
                    break 'read_loop;
                }
            }

            _ = shutdown_rx.recv() => {
                conn.shutdown();
            }

            _ = srx.recv() => {
                break 'read_loop;
            }

            result_read = reader.read(conn.rbuf_mut()) => {
                let n = match result_read {
                    Err(err) => {
                        event.on_error(&conn, g::Err::IOReadFailed(format!("{err}"))).await;
                        conn.shutdown();
                        break 'read_loop;
                    }

                    Ok(0) => {
                        conn.shutdown();
                        break 'read_loop;
                    }

                    Ok(v) => v,
                };

                'pack_loop: loop {
                    let option_pkt = match conn.builder().parse(conn.rbuf(n)) {
                        Err(err) => {
                            event.on_error(&conn, err).await;
                            break 'read_loop;
                        }
                        Ok(v) => v,
                    };

                    let (pkt, next) = match option_pkt {
                        None => break 'pack_loop,
                        Some(v) => v,
                    };

                    conn.recv_seq_incr();
                    conn.set_idempotent(pkt.idempotent());
                    if event.on_process(&conn, pkt).await.is_err() {
                        break 'read_loop;
                    }

                    if !next {
                        break 'pack_loop;
                    }
                }
            }
        }
    }

    writer_future.await.unwrap();
    drop(permit);
    event.on_disconnected(&conn).await;
    conn.reset();
}

async fn conn_write_handle<'a, T: super::server::IEvent>(
    conn: Ptr<'_, T::U>,
    event: T,
    mut writer: tokio::net::tcp::OwnedWriteHalf, 
    mut srx: tokio::sync::broadcast::Receiver<u8>) {
    'write_loop: loop {
        select! {
            _ = srx.recv() => {
                break 'write_loop;
            }

            option_out = conn.rx().next() => {
                if let Some(out) = option_out {
                    if let Err(err) = writer.write_all(out.raw()).await {
                        event.on_error(&conn, g::Err::IOWriteFailed(format!("{err}"))).await;
                        conn.shutdown();
                        break 'write_loop;
                    }
                    conn.send_seq_incr();
                }
            }
        }
    }
}

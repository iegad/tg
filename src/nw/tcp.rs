use super::{pack, IServerEvent, Server};
use crate::g;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::AsRawSocket;
use std::sync::{atomic::Ordering, Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream},
    select,
    sync::{broadcast, OwnedSemaphorePermit},
};

// ---------------------------------------------- Server run ----------------------------------------------
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
pub async fn server_run<T>(
    server: Arc<Server<T>>,
    conn_pool: &'static LinearObjectPool<super::Conn<T::U>>,
) -> g::Result<()>
where
    T: IServerEvent,
    T::U: Default + Sync + Send,
{
    // step 1: init tcp listener.
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

    // step 2: change server's state to running.
    if server.running.swap(true, Ordering::SeqCst) {
        return Err(g::Err::ServerIsAlreadyRunning);
    }

    // step 3: get shutdown sender and receiver
    let mut shutdown_rx = server.shutdown_tx.subscribe();

    // step 4: trigge server running event.
    server.event.on_running(&server).await;

    // step 5: accept_loop.
    let timeout = server.timeout;

    'accept_loop: loop {
        let event = server.event.clone();
        let shutdown = shutdown_rx.resubscribe();
        let conn = conn_pool.pull();

        select! {
            // when recv shutdown signal.
            _ = shutdown_rx.recv() => {
                tracing::debug!("accept_loop is breaking...");
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
                    conn_handle(stream, conn, timeout, shutdown, event, permit).await;
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
/// `conn_resuable` [LinearReusable<'static, super::Conn<T::U>>]
///
/// `timeout` read timeout
///
/// `shutdown_rx` server shutdown signal receiver.
///
/// `event` IEvent implement.
///
async fn conn_handle<T: IServerEvent>(
    mut stream: TcpStream,
    mut conn_lr: LinearReusable<'static, super::Conn<T::U>>,
    timeout: u64,
    mut shutdown_rx: broadcast::Receiver<u8>,
    event: T,
    permit: OwnedSemaphorePermit,
) {
    // step 1: active Conn<U>.
    let (w_tx, mut w_rx, mut conn_shutdown_rx) = conn_lr.setup(&stream);
    let conn = Arc::new(conn_lr);

    // step 2: get socket reader and writer
    let (mut reader, mut writer) = stream.split();

    // step 3: timeout ticker.
    let interval = if timeout == 0 {
        std::time::Duration::from_secs(60 * 60)
    } else {
        std::time::Duration::from_secs(timeout)
    };

    let mut ticker = tokio::time::interval(interval);

    // step 4: get request instance.
    let mut req = pack::PACK_POOL.pull();

    // step 5: triger connected event.
    if let Err(_) = event.on_connected(&conn).await {
        return;
    }

    // step 6: conn_loop.
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
                if timeout > 0 {
                    event.on_error(&conn, g::Err::TcpReadTimeout).await;
                    break 'conn_loop;
                }
            }

            // get response from write channel to write to remote.
            result_wbuf = w_rx.recv() => {
                let wbuf = match result_wbuf {
                    Err(err) => {
                        tracing::error!("wch recv failed: {:?}", err);
                        continue;
                    }
                    Ok(v) => v,
                };

                if let Err(err) = writer.write_all(&wbuf).await {
                    event.on_error(&conn, g::Err::TcpWriteFailed(format!("{err}"))).await;
                    break 'conn_loop;
                }
                conn.send_seq_incr();
            }

            // connection read data.
            result_read = reader.read(conn.rbuf_mut()) => {
                let nread = match result_read {
                    Err(err) => {
                        event.on_error(&conn, g::Err::TcpReadFailed(format!("{err}"))).await;
                        break 'conn_loop;
                    }
                    Ok(0) => break 'conn_loop,
                    Ok(v) => v,
                };

                tracing::debug!("读取 buf 长度: {}", nread);

                let mut consume = 0;
                'pack_loop: loop {
                    if req.valid() {
                        tracing::debug!("处理消息.");
                        conn.recv_seq_incr();
                        let option_rsp = match event.on_process(&conn, &req).await {
                            Err(_) => break 'conn_loop,
                            Ok(v) => v,
                        };

                        req.reset();
                        if let Some(rsp_bytes) = option_rsp {
                            if let Err(err) = w_tx.send(rsp_bytes) {
                                tracing::error!("w_tx.send failed: {err}");
                            }
                        }
                    } else {
                        consume += match req.from_bytes(&conn.rbuf()[consume..nread]) {
                            Err(err) => {
                                event.on_error(&conn, err).await;
                                break 'conn_loop;
                            }
                            Ok(v) => v,
                        };
                    }

                    if consume == nread && !req.valid() {
                        break 'pack_loop;
                    }
                }
            }
        }
    }

    // step 7: trigger disconnected event.
    drop(permit);
    event.on_disconnected(&conn).await;
}


// ---------------------------------------------- client run ----------------------------------------------
//
//
pub async fn client_run<T: super::IClientEvent>(host: &'static str, cli: Arc<super::Client<T::U>>) {
    let event = T::default();

    let mut stream = match TcpStream::connect(host).await {
        Err(err) => {
            event
                .on_error(&cli, g::Err::TcpConnectFailed(format!("{err}")))
                .await;
            return;
        }
        Ok(v) => v,
    };

    unsafe {
        let cli = &mut *(cli.as_ref() as *const super::Client<T::U> as *mut super::Client<T::U>);
        cli.remote = stream.peer_addr().unwrap();
        cli.local = stream.local_addr().unwrap();
        #[cfg(windows)]
        {
            cli.sockfd = stream.as_raw_socket();
        }
        #[cfg(unix)]
        {
            cli.sockfd = stream.as_raw_fd();
        }
    }

    if let Err(_) = event.on_connected(&cli).await {
        return;
    }

    let mut req = pack::REQ_POOL.pull();
    let (mut shutdown_rx, 
        wbuf_consumer, 
        w_tx, 
        mut w_rx) = cli.setup();
    let (mut reader, mut writer) = stream.split();
    let interval = if cli.timeout > 0 {
        cli.timeout
    } else {
        60 * 60
    };
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval));

    'cli_loop: loop {
        select! {
            // close client
            _ = shutdown_rx.recv() => {
                break 'cli_loop;
            }

            // timoeut
            _ = ticker.tick() => {
                if cli.timeout > 0 {
                    // TODO: ping
                }
            }

            // response write
            result_rsp = w_rx.recv() => {
                let rsp = match result_rsp {
                    Err(err) => {
                        tracing::error!("{err}");
                        break 'cli_loop;
                    }
                    Ok(v) => v,
                };

                if let Err(err) = writer.write_all(&rsp).await {
                    event.on_error(&cli, g::Err::TcpWriteFailed(format!("{err}"))).await;
                    break 'cli_loop;
                }
                cli.send_seq_incr();
            }

            // wbuf consumer
            result_wbuf = wbuf_consumer.recv() => {
                let wbuf = match result_wbuf {
                    Err(err) => {
                        tracing::error!("{err}");
                        break 'cli_loop;
                    }
                    Ok(v) => v,
                };

                if let Err(_) = w_tx.send(wbuf) {
                    tracing::error!("w_tx send failed");
                    break 'cli_loop;
                }
            }

            // read package
            result_read = reader.read(cli.rbuf_mut()) => {
                match result_read {
                    Err(err) => {
                        event.on_error(&cli, g::Err::TcpReadFailed(format!("{err}"))).await;
                        break 'cli_loop;
                    }
                    Ok(0) => break 'cli_loop,
                    Ok(_) => (),
                }

                'pack_loop: loop {
                    if req.valid() {
                        cli.recv_seq_incr();
                        let option_rsp = match event.on_process(&cli, &req).await {
                            Err(_) => break 'cli_loop,
                            Ok(v) => v,
                        };

                        cli.set_idempotent(req.idempotent());
                        req.reset();
                        if let Some(rsp) = option_rsp {
                            if let Err(_) = w_tx.send(rsp) {
                                tracing::error!("w_tx send failed");
                                break 'cli_loop;
                            }
                        }
                    } else {
                        if let Err(err) = req.from_bytes(cli.rbuf()) {
                            event.on_error(&cli, err).await;
                            break 'cli_loop;
                        }
                    }

                    if !req.valid() && cli.rbuf().len() < pack::Package::HEAD_SIZE {
                        break 'pack_loop;
                    }
                }
            }
        }
    }

    event.on_disconnected(&cli);
}

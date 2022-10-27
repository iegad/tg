use super::{server::{Server, self}, client, conn};
use futures::StreamExt;
use crate::g;
use std::{sync::{atomic::Ordering, Arc}, net::SocketAddr};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream, tcp::OwnedWriteHalf},
    select,
    sync::broadcast,
};

// ---------------------------------------------- server_run ----------------------------------------------

/// run a tcp server.
///
/// # Parameters
///
/// `server` Arc<ServerPtr<T>> instance.
///
/// `conn_pool` Conn<T::U> object pool.
pub async fn server_run<'a, T>(server: Arc<Server<T>>, conn_pool: &'static conn::Pool<'a, T::U>) -> g::Result<()>
where
    T: super::server::IEvent,
    T::U: Default + Sync + Send,
{
    let laddr: SocketAddr = server.host().parse().expect("host is invalid");

    // step 1: 初始化监听套接字
    let sockfd = if laddr.is_ipv4() {
        match TcpSocket::new_v4() {
            Err(err) => return Err(g::Err::SocketErr(format!("{err}"))),
            Ok(v) => v,
        }
    } else {
        match TcpSocket::new_v6() {
            Err(err) => return Err(g::Err::SocketErr(format!("{err}"))),
            Ok(v) => v,
        }
    };

    // --------------------------
    // SO_REUSEPORT和SO_REUSEADDR
    //
    // * SO_REUSEPORT是允许多个socket绑定到同一个ip+port上. SO_REUSEADDR用于对TCP套接字处于TIME_WAIT状态下的socket, 才可以重复绑定使用.
    //
    // * 两者使用场景完全不同.
    //
    //    SO_REUSEADDR 这个套接字选项通知内核, 如果端口忙, 但TCP状态位于TIME_WAIT, 可以重用端口. 
    //        这个一般用于当你的程序停止后想立即重启的时候, 如果没有设定这个选项, 会报错EADDRINUSE, 需要等到TIME_WAIT结束才能重新绑定到同一个 ip + port 上. 
    //
    //    SO_REUSEPORT 用于多核环境下, 允许多个线程或者进程绑定和监听同一个 ip + port, 无论UDP, TCP(以及TCP是什么状态).
    #[cfg(unix)]
    {
        if let Err(err) = SocketErr.set_reuseport(true) {
            return Err(g::Err::SocketErr(format!("{err}")));
        }
    }

    if let Err(err) = sockfd.set_reuseaddr(true) {
        return Err(g::Err::SocketErr(format!("{err}")));
    }

    if let Err(err) = sockfd.bind(laddr) {
        return Err(g::Err::SocketErr(format!("{err}")));
    }

    let listener = match sockfd.listen(1024) {
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
    let mut res: g::Result<()> = Ok(());

    'accept_loop: loop {
        let event = server.event.clone();
        let shutdown_rx_ = shutdown_rx.resubscribe();
        let conn = Arc::new(conn_pool.pull());

        select! {
            // when recv shutdown signal.
            _ = shutdown_rx.recv() => {
                break 'accept_loop;
            }

            // when connection comming.
            result_accept = listener.accept() => {
                let stream = match result_accept {
                    Err(err) => {
                        res = Err(g::Err::ServerAcceptError(format!("{err}")));
                        server.shutdown();
                        break 'accept_loop;
                    }
                    Ok((v, _)) => v
                };

                let permit = server.limit_connections.clone().acquire_owned().await.unwrap();
                tokio::spawn(async move {
                    conn_handle(stream, conn, timeout, shutdown_rx_, event).await;
                    drop(permit);
                });
            }
        }
    }

    // step 6: set server state running(false).
    server.running.store(false, Ordering::SeqCst);

    // step 7: trigger server stopped event.
    server.event.on_stopped(&server).await;

    res
}

// ---------------------------------------------- conn handle ----------------------------------------------

/// # conn_handle<T>
///
/// tcp connection's handler.
///
/// # Parameters
///
/// `stream` [TcpStream]
///
/// `conn` [LinearReusable<'static, Conn<T::U>>]
///
/// `timeout` read timeout
///
/// `s_down_rx` server shutdown signal receiver.
///
/// `event` IEvent implement.
///
async fn conn_handle<'a, T: super::server::IEvent>(
    stream: TcpStream,
    conn: conn::Ptr<'_, T::U>,
    timeout: u64,
    mut s_down_rx: broadcast::Receiver<u8>,
    event: T
) {
    // step 1: get socket reader and writer
    let (mut reader, writer) = stream.into_split();
    let mut c_down_rx = conn.load(&reader);
    let c_down_rx_ = c_down_rx.resubscribe();
    let event_ = event.clone();
    let conn_ = conn.clone();

    let w_fut = tokio::spawn(async move {
        conn_write_handle(conn_, event_, writer, c_down_rx_).await;
    });

    // step 3: timeout ticker.
    let mut ticker = tokio::time::interval(
        std::time::Duration::from_secs(if timeout == 0 { 60 * 60 } else { timeout })
    );

    if event.on_connected(&conn).await.is_err() {
        return;
    }

    'read_loop: loop {
        ticker.reset();

        select! {
            // 超时信号
            _ = ticker.tick() => {
                if timeout > 0 {
                    event.on_error(&conn, g::Err::IOReadTimeout).await;
                    break 'read_loop;
                }
            }

            // 服务端关闭信号
            _ = s_down_rx.recv() => {
                break 'read_loop;
            }

            // 会话端关闭信号
            _ = c_down_rx.recv() => {
                break 'read_loop;
            }

            // 会话端读消息信号
            result_read = reader.read(conn.rbuf_mut()) => {
                let n = match result_read {
                    Err(err) => {
                        event.on_error(&conn, g::Err::IOReadFailed(format!("{err}"))).await;
                        break 'read_loop;
                    }

                    Ok(0) => {
                        break 'read_loop;
                    }

                    Ok(v) => v,
                };

                'pack_loop: loop {
                    let option_inpk = match conn.builder().parse(conn.rbuf(n)) {
                        Err(err) => {
                            event.on_error(&conn, err).await;
                            break 'read_loop;
                        }
                        Ok(v) => v,
                    };

                    let (inpk, next) = match option_inpk {
                        None => break 'pack_loop,
                        Some(v) => v,
                    };

                    let idempotent = inpk.idempotent();
                    if idempotent > conn.idempotent() {
                        conn.recv_seq_incr();
                        if event.on_process(&conn, inpk).await.is_err() {
                            break 'read_loop;
                        }
                        conn.set_idempotent(idempotent);
                    }

                    if !next {
                        break 'pack_loop;
                    }
                }
            }
        }
    }

    conn.shutdown();
    w_fut.await.unwrap();
    event.on_disconnected(&conn).await;
}

// ---------------------------------------------- conn write handle ----------------------------------------------

/// 会话端写协程
/// 
/// 使用两个协程的目的是为了读写分离, 提高吞吐量.
async fn conn_write_handle<'a, T: super::server::IEvent>(
    conn: conn::Ptr<'_, T::U>,
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
                        break 'write_loop;
                    }
                    conn.send_seq_incr();
                }
            }
        }
    }
}

// ---------------------------------------------- conn write handle ----------------------------------------------

/// 运行TCP 客户端
pub async fn client_run<T: client::IEvent>(host: &str, cli: client::Ptr<'static, T::U>) {
    // step 1: 初始化事件实例
    let event = T::default();

    // step 2: 连接服务端
    let stream = match TcpStream::connect(host).await {
        Err(err) => {
            event.on_error(&cli, g::Err::TcpConnectFailed(format!("{err}"))).await;
            return;
        },
        Ok(v) => v,
    };
    let (mut reader, writer) = stream.into_split();

    // step 3: 准备读协程数据
    let (mut shutdown_rx, rx) = cli.load(&reader);
    let builder = cli.builder();
    let mut rbuf = [0u8; g::DEFAULT_BUF_SIZE];

    // step 4: 准备写协程数据
    let shutdown_rx_ = shutdown_rx.resubscribe();
    let event_ = event.clone();
    let cli_ = cli.clone();

    // step 5: 开启写协程
    let w_fut = tokio::spawn(async move {
        cli_write_handle(cli_, event_, writer, shutdown_rx_, rx).await;
    });

    // step 6: 连接事件触发
    if event.on_connected(&cli).await.is_err() {
        return;
    }

    // step 7: 开启读循环, 客户端是不设定读超时的.
    'read_loop: loop {
        select! {
            // 关闭信号
            _ = shutdown_rx.recv() => {
                break 'read_loop;
            }

            // 读消息信号
            result_read = reader.read(&mut rbuf) => {
                let n = match result_read {
                    Err(err) => {
                        event.on_error(&cli, g::Err::IOReadFailed(format!("{err}"))).await;
                        break 'read_loop;
                    }

                    Ok(v) => v,
                };

                'pack_loop: loop {
                    let option_pkt = match builder.parse(&rbuf[..n]) {
                        Ok(v) => v,
                        Err(err) => {
                            event.on_error(&cli, err).await;
                            break 'read_loop;
                        }
                    };

                    let (pkt, next) = match option_pkt {
                        None => break 'pack_loop,
                        Some(v) => v,
                    };

                    let idempotent = pkt.idempotent();
                    if idempotent > cli.idempotent() {
                        cli.recv_seq_incr();
                        if event.on_process(&cli, pkt).await.is_err() {
                            break 'read_loop;
                        }
                        cli.set_idempotent(idempotent);
                    }

                    if !next {
                        break 'pack_loop;
                    }
                }
            }
        }
    }

    cli.shutdown();
    w_fut.await.unwrap();
    event.on_disconnected(&cli).await;
}

// ---------------------------------------------- conn write handle ----------------------------------------------

/// 客户端写协程
async fn cli_write_handle<T: client::IEvent>(
    cli: client::Ptr<'static, T::U>,
    event: T,
    mut writer: OwnedWriteHalf, 
    mut shutdown_rx: broadcast::Receiver<u8>,
    rx: async_channel::Receiver<server::Packet>) {

    'write_loop: loop {
        select! {
            _ = shutdown_rx.recv() => {
                break 'write_loop;
            }

            result_pkt = rx.recv() => {
                let pkt = match result_pkt {
                    Err(err) => {
                        event.on_error(&cli, g::Err::AsyncRecvError(format!("{err}"))).await;
                        break 'write_loop;
                    }
                    Ok(v) => v,
                };

                if let Err(err) = writer.write_all(pkt.raw()).await {
                    event.on_error(&cli, g::Err::IOWriteFailed(format!("{err}"))).await;
                    break 'write_loop;
                }
                cli.send_seq_incr();
            }
        }
    }
}
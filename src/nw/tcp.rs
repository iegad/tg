use super::{conn::CONN_POOL, pack, IProc, Server};
use crate::g;
use bytes::BytesMut;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select, signal,
    sync::broadcast,
};

/// # run
///
/// 运行server
pub async fn run<TProc>(server: &Server, proc: TProc)
where
    TProc: IProc,
{
    proc.on_init(server).await;

    let listener = match TcpListener::bind(server.host).await {
        Ok(l) => l,
        Err(err) => panic!("tcp server bind failed: {:?}", err),
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
            // 接收连接句柄
            res_accept = listener.accept() => {
                let (stream, addr) = match res_accept {
                    Ok((s, addr)) => (s,addr),
                    Err(err) => {
                        println!("accept failed: {:?}", err);
                        break 'server_loop;
                    }
                };

                let shutdown = notify_shutdown.subscribe();

                tokio::spawn(async move {
                    conn_handle(stream, addr, proc, shutdown).await;
                    drop(permit);
                });
            }

            // SIGINT 信号句柄
            _ = signal::ctrl_c() => {
                if let Err(err) = notify_shutdown.send(1) {
                    panic!("shutdown_sender.send failed: {:?}", err);
                }
            }

            // 关停句柄
            _ = shutdown.recv() => {
                break 'server_loop;
            }
        }
    }

    proc.on_released(server).await;
}

async fn conn_handle<TProc>(
    mut stream: TcpStream,
    _addr: SocketAddr,
    proc: TProc,
    mut notify_shutdown: broadcast::Receiver<u8>,
) where
    TProc: IProc,
{
    let mut conn = CONN_POOL.pull();
    conn.from_with(&stream);

    if let Err(err) = proc.on_connected(&*conn).await {
        println!("proc.on_connected failed: {:?}", err);
        return;
    }

    let (mut reader, mut writer) = stream.split();
    let mut timeout_ticker = tokio::time::interval(Duration::from_secs(g::DEFAULT_READ_TIMEOUT));
    let tx = conn.sender();
    let mut rx = conn.receiver();
    let mut req = pack::PACK_POOL.pull();

    'conn_loop: loop {
        timeout_ticker.reset();

        select! {
            // 关停句柄
            _ = notify_shutdown.recv() => {
                println!("server is shutdown");
                break 'conn_loop;
            }

            // 超时句柄
            _ = timeout_ticker.tick() => {
                proc.on_conn_error(&*conn, g::Err::TcpReadTimeout).await;
                break 'conn_loop;
            }

            // 消息发送句柄
            result_rsp = rx.recv() => {
                let rsp = match result_rsp {
                    Err(err) => panic!("failed wch rsp: {:?}", err),
                    Ok(v) => v,
                };

                if let Err(err) = writer.write_all(&rsp).await {
                    proc.on_conn_error(&*conn, g::Err::TcpWriteFailed(format!("write failed: {:?}", err))).await;
                    break 'conn_loop;
                }
            }

            // 消息接收句柄
            result_read = reader.read_buf(conn.rbuf_mut()) => {
                match result_read {
                    Ok(0) => {
                        println!("conn[{}:{:?}] closed", conn.sockfd(), conn.remote());
                        break 'conn_loop;
                    }

                    Ok(n) => n,

                    Err(err) => {
                        proc.on_conn_error(&*conn, g::Err::TcpReadFailed(format!("read failed: {:?}", err))).await;
                        break 'conn_loop;
                    }
                };

                let ok = match req.parse_buf(conn.rbuf_mut()) {
                    Err(err) => {
                        proc.on_conn_error(&*conn, err).await;
                        break 'conn_loop;
                    }

                    Ok(v) => v,
                };

                if ok {
                    conn.recv_seq_incr();
                    let wbuf = match proc.on_process(&*conn, &req).await {
                        Err(_) => break 'conn_loop,
                        Ok(v) => v,
                    };

                    if let Err(err) = tx.send(wbuf) {
                        panic!("write channel[mpsc] send failed: {}", err);
                    }
                }
            }
        }
    }
    proc.on_disconnected(&*conn).await;
}

pub async fn read(
    sock: &mut TcpStream,
    rbuf: &mut BytesMut,
    pack: &mut pack::Package,
) -> g::Result<usize> {
    loop {
        match sock.read_buf(rbuf).await {
            Ok(0) => {
                return Ok(0);
            }

            Ok(n) => n,
            Err(err) => {
                return Err(g::Err::TcpReadFailed(format!("{:?}", err)));
            }
        };

        if pack.parse_buf(rbuf)? {
            return Ok(1);
        }
    }
}

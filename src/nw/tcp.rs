use super::{
    conn,
    pack::{self},
    ICliProc, Server,
};
use crate::g;
use bytes::BytesMut;
use lockfree_object_pool::LinearObjectPool;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select, signal,
    sync::broadcast,
};

lazy_static::lazy_static! {
    /// 读缓冲区池
    static ref RBUF_POOL: LinearObjectPool<BytesMut> = LinearObjectPool::new(||BytesMut::with_capacity(g::DEFAULT_BUF_SIZE), |v|{v.clear()});
}

/// 运行 tcp server
pub async fn server_run<TProc: super::ISvrProc>(server: &Server, proc: TProc) {
    proc.on_init(server).await;
    // step 1: 构建 tcp listener
    let listener = match TcpListener::bind(server.host).await {
        Ok(l) => l,
        Err(err) => panic!("tcp server bind failed: {:?}", err),
    };

    // step 2: 构建 signal channel
    let (notify_shutdown, mut shutdown) = broadcast::channel(1);

    // step 3: 启动 轮询服务
    //    两种情况下会退出轮训:
    //    1: listener.accept出现错误.
    //    2: 收到 SIGINT 信息.
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

// tcp conn 句柄
async fn conn_handle<TProc: super::ISvrProc>(
    mut stream: TcpStream,
    _addr: SocketAddr,
    proc: TProc,
    mut notify_shutdown: broadcast::Receiver<u8>,
) {
    // step 1: 初始化 connection
    let mut conn = conn::CONN_POOL.pull();
    conn.init(&stream);

    if let Err(err) = proc.on_connected(&*conn).await {
        println!("proc.on_connected failed: {:?}", err);
        return;
    }

    // step 2: 构建相关对象
    //    1) tcp reader/writer
    //    2) read timeout
    //    3) tx [sender] 消息发送管道
    //    4) req 请求包
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
                proc.on_conn_error(&conn, g::Err::TcpReadTimeout).await;
                break 'conn_loop;
            }

            // 消息发送句柄
            result_rsp = rx.recv() => {
                let rsp = match result_rsp {
                    Err(err) => panic!("failed wch rsp: {:?}", err),
                    Ok(v) => v,
                };

                if let Err(err) = writer.write_all(&rsp).await {
                    proc.on_conn_error(&conn, g::Err::TcpWriteFailed(format!("write failed: {:?}", err))).await;
                    break 'conn_loop;
                }

                conn.send_seq_incr();
            }

            // 消息接收句柄
            result_read = reader.read_buf(req.rbuf_mut()) => {
                match result_read {
                    // EOF
                    Ok(0) => break 'conn_loop,

                    // IO错误 或 非法 消息
                    Ok(n) => {
                        if n < pack::Package::HEAD_SIZE {
                            proc.on_conn_error(&conn, g::Err::PackHeadInvalid("head size is invalid")).await;
                            break 'conn_loop;
                        }
                    }

                    Err(err) => {
                        proc.on_conn_error(&conn, g::Err::TcpReadFailed(format!("read failed: {:?}", err))).await;
                        break 'conn_loop;
                    }
                };

                match req.parse() {
                    Err(err) => {
                        proc.on_conn_error(&conn, err).await;
                        break 'conn_loop;
                    }

                    Ok(ok) => {
                        if ok {
                            if conn.idempotent() >= req.idempotent() {
                                continue 'conn_loop;
                            }

                            conn.recv_seq_incr();

                            let some_wbuf = match proc.on_process(&conn, &req).await {
                                Err(_) => break 'conn_loop,
                                Ok(v) => v,
                            };

                            conn.set_idempotent(req.idempotent());

                            if let Some(wbuf) = some_wbuf {
                                if let Err(err) = tx.send(wbuf) {
                                    panic!("write channel[mpsc] send failed: {}", err);
                                }
                            }
                        }
                    },
                };
            }
        }
    }
    proc.on_disconnected(&conn).await;
}

/// tcp 读消息
///
/// 成功读取消息包 返回 true.
/// 连接断开 返回 false.
/// 否则返回相应错误.
pub async fn read(sock: &mut TcpStream, pack: &mut pack::Package) -> g::Result<bool> {
    let mut rbuf = RBUF_POOL.pull();
    loop {
        match sock.read_buf(&mut *rbuf).await {
            Ok(0) => {
                return Ok(false);
            }

            Ok(n) => n,
            Err(err) => {
                return Err(g::Err::TcpReadFailed(format!("{:?}", err)));
            }
        };

        if pack.parse_buf(&mut rbuf)? {
            return Ok(true);
        }
        rbuf.clear();
    }
}

pub struct Client {
    idempotent: u32,
    send_seq: u32,
    recv_seq: u32,
    stream: TcpStream,
    tx: broadcast::Sender<BytesMut>,
}

impl Client {
    pub async fn new(host: &str) -> g::Result<Self> {
        let stream = match TcpStream::connect(host).await {
            Err(err) => return Err(g::Err::TcpConnectFailed(format!("{:?}", err))),
            Ok(v) => v,
        };

        let (tx, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);
        Ok(Self {
            idempotent: 0,
            send_seq: 0,
            recv_seq: 0,
            stream,
            tx,
        })
    }

    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    pub fn stream_mut(&mut self) -> &TcpStream {
        &self.stream
    }

    pub fn sender(&self) -> broadcast::Sender<BytesMut> {
        self.tx.clone()
    }

    pub fn receiver(&self) -> broadcast::Receiver<BytesMut> {
        self.tx.subscribe()
    }
}

pub async fn client_run<TProc: ICliProc>(
    mut cli: Client,
    mut shutdown: broadcast::Receiver<u8>,
    proc: TProc,
) {
    let tx = cli.sender();
    let mut rx = cli.receiver();
    let (mut reader, mut writer) = cli.stream.split();
    let mut req = pack::PACK_POOL.pull();

    'cli_loop: loop {
        select! {
            _ = shutdown.recv() => break 'cli_loop,

            result_wbuf = rx.recv() => {
                let wbuf = match result_wbuf {
                    Ok(v) => v,
                    Err(err) => panic!("wch recv failed: {:?}", err),
                };

                if let Err(err) = writer.write_all(&wbuf).await {
                    proc.on_error(&g::Err::TcpWriteFailed(format!("{:?}", err))).await;
                    break 'cli_loop;
                }
            }

            result_read = reader.read_buf(req.rbuf_mut()) => {
                match result_read {
                    Ok(0) => break 'cli_loop,
                    Ok(_) => {}
                    Err(err) => {
                        proc.on_error(&g::Err::TcpReadFailed(format!("{:?}", err))).await;
                        break 'cli_loop;
                    }
                }

                match req.parse() {
                    Err(err) => {
                        proc.on_error(&err).await;
                        break 'cli_loop;
                    }
                    Ok(ok) => {
                        if ok {
                            match proc.on_process(&req).await {
                                Err(_) => break 'cli_loop,
                                Ok(some_wbuf) => {
                                    if let Some(wbuf) = some_wbuf {
                                        if let Err(err) = tx.send(wbuf) {
                                            panic!("wch send failed: {:?}", err);
                                        }
                                    }
                                }
                            };
                        }
                    }
                }
            }
        }
    }
}

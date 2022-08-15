use crate::g;
use std::{net::SocketAddr, os::unix::prelude::AsRawFd, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select, signal,
    sync::{broadcast, Semaphore},
};

use super::{iface, pack};

pub struct Conn {
    sockfd: i32,
    send_seq: u32,
    recv_seq: u32,
    remote: SocketAddr,
    local: SocketAddr,
}

impl Conn {
    pub fn new(stream: &TcpStream) -> g::Result<Conn> {
        if let Err(_) = stream.set_nodelay(true) {
            return Err(g::Err::TcpSetOptionFailed("set_nodelay"));
        }

        let remote = match stream.peer_addr() {
            Ok(addr) => addr,
            Err(_) => return Err(g::Err::TcpGetRemoteAddrFailed),
        };

        let local = match stream.local_addr() {
            Ok(addr) => addr,
            Err(_) => return Err(g::Err::TcpGetLocalAddrFailed),
        };

        let sockfd = stream.as_raw_fd();

        Ok(Conn {
            sockfd,
            send_seq: 0,
            recv_seq: 0,
            remote,
            local,
        })
    }
}

impl iface::IConn for Conn {
    fn remote(&self) -> &SocketAddr {
        &self.remote
    }

    fn local(&self) -> &SocketAddr {
        &self.local
    }

    fn sockfd(&self) -> i32 {
        self.sockfd
    }

    fn send_seq(&self) -> u32 {
        self.send_seq
    }

    fn recv_seq(&self) -> u32 {
        self.recv_seq
    }
}

pub struct Server {
    max_connections: usize,
    limit_connections: Arc<Semaphore>,
    host: SocketAddr,
}

impl Server {
    pub fn new(host: &str, max_connections: usize) -> g::Result<Server> {
        let host: SocketAddr = match host.parse() {
            Ok(addr) => addr,
            Err(_) => return Err(g::Err::TcpSocketAddrInvalid(host.to_string())),
        };

        let max_connections = if max_connections == 0 {
            g::DEFAULT_MAX_CONNECTIONS
        } else {
            max_connections
        };

        Ok(Server {
            max_connections,
            limit_connections: Arc::new(Semaphore::new(max_connections)),
            host,
        })
    }
}

impl iface::IServer for Server {
    fn host(&self) -> &SocketAddr {
        &self.host
    }

    fn current_connections(&self) -> usize {
        self.max_connections - self.limit_connections.available_permits()
    }

    fn max_connections(&self) -> usize {
        self.max_connections
    }
}

impl Server {
    pub async fn run<TProc>(&self, proc: TProc, notify_shutdown: broadcast::Sender<u8>)
    where
        TProc: iface::IProc,
    {
        if let Err(err) = proc.on_init(self) {
            println!("proc.on_init failed: {:?}", err);
            return;
        }

        let listener = match TcpListener::bind(self.host).await {
            Ok(l) => l,
            Err(err) => panic!("tcp server bind failed: {:?}", err),
        };

        let mut shutdown = notify_shutdown.subscribe();

        'server_loop: loop {
            let permit = self
                .limit_connections
                .clone()
                .acquire_owned()
                .await
                .unwrap();

            select! {
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
                        Self::conn_handle(stream, addr, proc, shutdown).await;
                        drop(permit);
                    });
                }

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

        proc.on_released(self);
    }

    async fn conn_handle<TProc>(
        mut stream: TcpStream,
        _addr: SocketAddr,
        proc: TProc,
        mut notify_shutdown: broadcast::Receiver<u8>,
    ) where
        TProc: iface::IProc,
    {
        let conn = match Conn::new(&stream) {
            Ok(c) => c,
            Err(err) => {
                println!("conn.new failed: {:?}", err);
                return;
            }
        };

        if let Err(err) = proc.on_connected(&conn) {
            println!("proc.on_connected failed: {:?}", err);
            return;
        }

        let (mut reader, mut writer) = stream.split();
        let (wch_sender, mut wch_receiver) = tokio::sync::mpsc::channel(g::DEFAULT_CHAN_SIZE);
        let mut timeout_ticker =
            tokio::time::interval(std::time::Duration::from_secs(g::DEFAULT_READ_TIMEOUT));
        let mut rbuf = [0u8; 1500];
        let mut in_pack = pack::Package::new();

        'conn_loop: loop {
            timeout_ticker.reset();

            select! {
                _ = notify_shutdown.recv() => {
                    println!("server is shutdown");
                    break 'conn_loop;
                }

                _ = timeout_ticker.tick() => {
                    proc.on_conn_error(&conn, g::Err::TcpReadTimeout);
                    break 'conn_loop;
                }

                result_rsp = wch_receiver.recv() => {
                    let rsp: Vec<u8> = match result_rsp {
                        None => {
                            panic!("failed wch rsp");
                        }
                        Some(v) => v,
                    };

                    if let Err(err) = writer.write_all(&rsp).await {
                        proc.on_conn_error(&conn, g::Err::TcpWriteFailed(format!("write failed: {:?}", err)));
                        break 'conn_loop;
                    }
                }

                result_read = reader.read(&mut rbuf) => {
                    let n = match result_read {
                        Ok(0) => {
                            println!("conn[{}:{:?}] closed", conn.sockfd, conn.remote);
                            break 'conn_loop;
                        }

                        Ok(n) => n,

                        Err(err) => {
                            proc.on_conn_error(&conn, g::Err::TcpReadFailed(format!("read failed: {:?}", err)));
                            break 'conn_loop;
                        }
                    };

                    let ok = match in_pack.parse(&rbuf[..n]) {
                        Err(err) => {
                            proc.on_conn_error(&conn, err);
                            break 'conn_loop;
                        }

                        Ok(v) => v,
                    };

                    if ok {
                        let rsp = match proc.on_process(&conn, &in_pack) {
                            Err(err) => {
                                proc.on_conn_error(&conn, err);
                                break 'conn_loop;
                            }

                            Ok(rsp) => rsp,
                        };

                        in_pack.clear();

                        if let Err(err) = wch_sender.send(rsp).await {
                            panic!("write channel[mpsc] send failed: {:?}", err);
                        }
                    }
                }
            }
        }
        proc.on_disconnected(&conn);
    }
}

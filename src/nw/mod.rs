pub mod pack;
pub mod tcp;
use crate::{g, us::Ptr};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
    os::unix::prelude::AsRawFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{
    net::TcpStream,
    sync::{broadcast, Semaphore},
};

// ----------------------------------- 工具函数 -----------------------------------
//
//
/// # sockaddr_to_bytes
///
/// 将 [std::net::SocketAddr] 转换成 IP字节和端口
///
/// IPV4: Vec<u8> 为  4 字节
/// IPV6: Vec<u8> 为 16 字节
///
/// # Example
///
/// ```
/// let (v, port) = tg::nw::sockaddr_to_bytes("0.0.0.0:6688".parse().unwrap()).unwrap();
/// assert_eq!(v.len(), 4);
/// assert_eq!(port, 6688);
/// ```
pub fn sockaddr_to_bytes(sock_addr: SocketAddr) -> g::Result<(Vec<u8>, u16)> {
    let ip = match sock_addr.ip() {
        IpAddr::V4(ip) => ip.octets().to_vec(),
        IpAddr::V6(ip) => ip.octets().to_vec(),
    };

    Ok((ip, sock_addr.port()))
}

/// bytes_to_sockaddr
///
/// 将 [(Vec<u8>, u16)] 转换成 [std::net::SocketAddr]
///
/// buf.len == [4 | 16];
///
/// ```
/// let ipv = vec![0u8; 4];
/// let sockaddr = tg::nw::bytes_to_sockaddr(&ipv, 6688).unwrap();
/// assert_eq!("0.0.0.0:6688", format!("{:?}", sockaddr));
/// ```
pub fn bytes_to_sockaddr(buf: &[u8], port: u16) -> g::Result<SocketAddr> {
    if port == 0 {
        return Err(g::Err::NwInvalidPort);
    }

    let addr: IpAddr = match buf.len() {
        4 => {
            let tmp: [u8; 4] = match buf[..4].try_into() {
                Ok(v) => v,
                Err(err) => return Err(g::Err::Custom(format!("{}", err))),
            };

            match tmp.try_into() {
                Ok(v) => v,
                Err(err) => return Err(g::Err::Custom(format!("{}", err))),
            }
        }

        16 => {
            let tmp: [u8; 16] = match buf[..16].try_into() {
                Ok(v) => v,
                Err(err) => return Err(g::Err::Custom(format!("{}", err))),
            };

            match tmp.try_into() {
                Ok(v) => v,
                Err(err) => return Err(g::Err::Custom(format!("{}", err))),
            }
        }

        _ => return Err(g::Err::NwInvalidIP),
    };

    Ok(SocketAddr::new(addr, port))
}

#[async_trait]
pub trait IEvent: Default + Send + Sync + Clone + 'static {
    async fn on_error(&self, conn: &Conn, err: g::Err) {
        tracing::debug!("[{}|{:?}] => {:?}", conn.sockfd, conn.remote(), err);
    }

    async fn on_connected(&self, conn: &Conn) -> g::Result<()> {
        tracing::debug!("[{}|{:?}] has connected", conn.sockfd, conn.remote());
        Ok(())
    }

    async fn on_disconnected(&self, conn: &Conn) {
        tracing::debug!("[{}|{:?}] has disconnected", conn.sockfd, conn.remote());
    }

    async fn on_process(&self, conn: &Conn, req: &pack::Package) -> g::Result<Option<Bytes>>;
}

#[async_trait]
pub trait IServerEvent: IEvent {
    async fn on_runing(&self, server: ServerPtr<Self>) {
        tracing::debug!(
            "server[HOST:{}|MAX:{}|TIMOUT:{}] is running...",
            server.host,
            server.max_connections,
            server.timeout
        );
    }

    async fn on_stopped(&self, server: ServerPtr<Self>) {
        tracing::debug!("server[HOST:{}] has stopped...!!!", server.host);
    }
}

pub struct Server<T> {
    host: &'static str,
    max_connections: usize,
    timeout: u64,
    limit_connections: Arc<Semaphore>,
    event: T,
    shutdown: broadcast::Sender<u8>,
    running: AtomicBool,
}

pub type ServerPtr<T> = Arc<Ptr<Server<T>>>;

impl<T: IServerEvent> Server<T> {
    pub fn new(host: &'static str, max_connections: usize, timeout: u64) -> ServerPtr<T> {
        let (shutdown, _) = broadcast::channel(1);

        Ptr::parse(Self {
            host,
            max_connections,
            timeout,
            limit_connections: Arc::new(Semaphore::new(max_connections)),
            event: T::default(),
            shutdown,
            running: AtomicBool::new(false),
        })
    }

    pub fn host(&self) -> &'static str {
        self.host
    }

    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    pub fn current_connections(&self) -> usize {
        self.max_connections - self.limit_connections.available_permits()
    }

    pub fn timeout(&self) -> u64 {
        self.timeout
    }

    pub fn shutdown(&self) {
        self.shutdown.send(1).unwrap();
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

impl<T: IServerEvent> Default for Server<T> {
    fn default() -> Self {
        let (shutdown, _) = broadcast::channel(1);

        Self {
            host: "0.0.0.0:8080",
            max_connections: g::DEFAULT_MAX_CONNECTIONS,
            timeout: g::DEFAULT_READ_TIMEOUT,
            limit_connections: Arc::new(Semaphore::new(g::DEFAULT_MAX_CONNECTIONS)),
            event: Default::default(),
            shutdown,
            running: AtomicBool::new(false),
        }
    }
}

pub struct Conn {
    #[cfg(unix)]
    sockfd: i32,
    idempoetnt: u32,
    send_seq: u32,
    recv_seq: u32,
    remote: SocketAddr,
    local: SocketAddr,
    wch_sender: broadcast::Sender<Bytes>,
    rbuf: BytesMut,
}

impl Conn {
    pub fn new() -> Self {
        let (wch_sender, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);
        Self {
            sockfd: 0,
            idempoetnt: 0,
            send_seq: 0,
            recv_seq: 0,
            remote: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            local: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            wch_sender,
            rbuf: BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        }
    }

    pub fn load_from(&mut self, stream: &TcpStream) {
        use nix::sys::socket;

        #[cfg(unix)]
        let sockfd = stream.as_raw_fd();

        stream.set_nodelay(true).unwrap();
        #[cfg(unix)]
        socket::setsockopt(sockfd, socket::sockopt::RcvBuf, &(1024 * 1024)).unwrap();
        #[cfg(unix)]
        socket::setsockopt(sockfd, socket::sockopt::SndBuf, &(1024 * 1024)).unwrap();

        self.sockfd = stream.as_raw_fd();
        self.remote = stream.peer_addr().unwrap();
        self.local = stream.local_addr().unwrap();
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.sockfd = 0;
        self.idempoetnt = 0;
        self.send_seq = 0;
        self.recv_seq = 0;
    }

    #[inline(always)]
    pub fn check_rbuf(&mut self) {
        let n = self.rbuf.len();
        if n < pack::Package::HEAD_SIZE {
            self.rbuf.resize(g::DEFAULT_BUF_SIZE, 0);
            unsafe { self.rbuf.set_len(n) };
        }
    }

    #[inline(always)]
    #[cfg(unix)]
    pub fn sockfd(&self) -> i32 {
        self.sockfd
    }

    #[inline(always)]
    pub fn remote(&self) -> &SocketAddr {
        &self.remote
    }

    #[inline(always)]
    pub fn local(&self) -> &SocketAddr {
        &self.local
    }

    #[inline(always)]
    pub fn receiver(&self) -> broadcast::Receiver<Bytes> {
        self.wch_sender.subscribe()
    }

    #[inline(always)]
    pub fn sender(&self) -> broadcast::Sender<Bytes> {
        self.wch_sender.clone()
    }

    #[inline(always)]
    pub fn rbuf_mut(&mut self) -> &mut BytesMut {
        &mut self.rbuf
    }

    #[inline(always)]
    pub fn rbuf(&self) -> &BytesMut {
        &self.rbuf
    }

    #[inline(always)]
    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    #[inline(always)]
    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }
}

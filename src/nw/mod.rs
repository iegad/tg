pub mod pack;
pub mod tcp;
pub mod tools;

use crate::{g, us::Ptr};
use async_trait::async_trait;
use bytes::BytesMut;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::{AsRawSocket, RawSocket};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{
    net::TcpStream,
    sync::{broadcast, Semaphore},
};

/// # Response
///
/// network's event process reply to connection data
pub type Response = Arc<LinearReusable<'static, BytesMut>>;

/// # ConnPtr<T>
pub type ConnPtr<T> = Arc<LinearReusable<'static, Conn<T>>>;

/// # IEvent
///
/// common network events
///
/// # Generic U
///
/// User's custom data.
///
/// # Example
///
/// ```
/// use async_trait::async_trait;
///
/// #[derive(Clone, Copy, Default)]
/// struct DemoEvent;
///
/// #[async_trait]
/// impl tg::nw::IEvent for DemoEvent {
///     // make IEvent::U => ().
///     type U = ();
///
///     async fn on_process(&self, conn: &tg::nw::Conn<()>, req: &tg::nw::pack::Package) -> tg::g::Result<Option<tg::nw::Response>> {
///         println!("{:?} => {:?}", conn.remote(), req.data());
///         Ok(None)
///     }
/// }
/// ```
#[async_trait]
pub trait IEvent: Default + Send + Sync + Clone + 'static {
    type U: Sync + Send + Default;

    /// connection's error event
    ///
    /// # Trigger
    ///
    /// after connection error.
    async fn on_error(&self, conn: &ConnPtr<Self::U>, err: g::Err) {
        tracing::error!("[{}|{:?}] => {:?}", conn.sockfd, conn.remote, err);
    }

    async fn on_connected(&self, conn: &ConnPtr<Self::U>) -> g::Result<()> {
        tracing::debug!("[{}|{:?}] has connected", conn.sockfd, conn.remote);
        Ok(())
    }

    /// connection's disconnected event
    ///
    /// # Trigger
    ///
    /// after connection has disconnected.
    async fn on_disconnected(&self, conn: &ConnPtr<Self::U>) {
        tracing::debug!("[{}|{:?}] has disconnected", conn.sockfd, conn.remote);
    }

    /// connection's has package received needs to be process.
    ///
    /// # Trigger
    ///
    /// after connection receive a complete package.
    async fn on_process(
        &self,
        conn: &ConnPtr<Self::U>,
        req: &pack::Package,
    ) -> g::Result<Option<Response>>;
}

/// # IServerEvent
///
/// server network events
///
/// # Example
///
/// ```
/// ```
#[async_trait]
pub trait IServerEvent: IEvent {
    /// server start event
    ///
    /// # Trigger
    ///
    /// before server start listening.
    async fn on_running(&self, server: ServerPtr<Self>) {
        tracing::debug!(
            "server[HOST:{}|MAX:{}|TIMOUT:{}] is running...",
            server.host,
            server.max_connections,
            server.timeout
        );
    }

    /// server stop event
    ///
    /// # Trigger
    ///
    /// after server has stopped listen.
    async fn on_stopped(&self, server: ServerPtr<Self>) {
        tracing::debug!("server[HOST:{}] has stopped...!!!", server.host);
    }
}

/// # Server<T>
///
/// common server option
///
/// # Generic T
///
/// IServerEvent implement.
pub struct Server<T> {
    event: T,
    host: &'static str,
    max_connections: usize,
    timeout: u64,
    running: AtomicBool,
    limit_connections: Arc<Semaphore>,
    shutdown: broadcast::Sender<u8>,
}

/// # ServerPtr<T>
///
/// `ServerPtr<T>` is `Arc<Ptr<Server<T>>>`'s nickname.
///
/// `Ptr` can let Server<T> mutable by immutable refrence.
pub type ServerPtr<T> = Arc<Ptr<Server<T>>>;

impl<T: IServerEvent> Server<T> {
    /// # Server<T>::new_ptr
    ///
    /// make a ServerPtr<T>.
    ///
    /// # Params
    ///
    /// `host` listen address.
    ///
    /// `max_connections` max connections.
    ///
    /// `timeout` connection read timeout, unit second(s).
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::Server;
    /// use async_trait::async_trait;
    ///
    /// #[derive(Copy, Clone, Default)]
    /// struct DemoEvent;
    ///
    /// #[async_trait]
    /// impl tg::nw::IEvent for DemoEvent {
    ///     type U = ();
    ///
    ///     async fn on_process(&self, conn: &tg::nw::Conn<()>, req: &tg::nw::pack::Package) -> tg::g::Result<Option<tg::nw::Response>> {
    ///         println!("{:?} => {:?}", conn.remote(), req.data());
    ///         Ok(None)
    ///     }
    /// }
    ///
    /// impl tg::nw::IServerEvent for DemoEvent {}
    ///
    /// let server = Server::<DemoEvent>::new_ptr("0.0.0.0:6688", 100, 60);
    /// assert_eq!(server.max_connections(), 100);
    /// assert_eq!(server.host(), "0.0.0.0:6688");
    /// ```
    pub fn new_ptr(host: &'static str, max_connections: usize, timeout: u64) -> ServerPtr<T> {
        Ptr::parse(Self::new(host, max_connections, timeout))
    }

    /// # Server<T>::new
    ///
    /// make a Server<T>
    ///
    /// # Params
    ///
    /// same as [Server<T>::new_ptr]
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::Server;
    /// use async_trait::async_trait;
    ///
    /// #[derive(Copy, Clone, Default)]
    /// struct DemoEvent;
    ///
    /// #[async_trait]
    /// impl tg::nw::IEvent for DemoEvent {
    ///     type U = ();
    ///
    ///     async fn on_process(&self, conn: &tg::nw::Conn<()>, req: &tg::nw::pack::Package) -> tg::g::Result<Option<tg::nw::Response>> {
    ///         println!("{:?} => {:?}", conn.remote(), req.data());
    ///         Ok(None)
    ///     }
    /// }
    ///
    /// impl tg::nw::IServerEvent for DemoEvent {}
    ///
    /// let server = Server::<DemoEvent>::new("0.0.0.0:6688", 100, 60);
    /// assert_eq!(server.max_connections(), 100);
    /// assert_eq!(server.host(), "0.0.0.0:6688");
    /// ```
    pub fn new(host: &'static str, max_connections: usize, timeout: u64) -> Server<T> {
        let (shutdown, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);

        Self {
            host,
            max_connections,
            timeout,
            limit_connections: Arc::new(Semaphore::new(max_connections)),
            event: T::default(),
            shutdown,
            running: AtomicBool::new(false),
        }
    }

    /// listen addres
    #[inline]
    pub fn host(&self) -> &'static str {
        self.host
    }

    /// max connections
    #[inline]
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// current connections
    #[inline]
    pub fn current_connections(&self) -> usize {
        self.max_connections - self.limit_connections.available_permits()
    }

    /// connection's read timeout
    #[inline]
    pub fn timeout(&self) -> u64 {
        self.timeout
    }

    /// server's state, if returns true means server is running. false means server is not running.
    #[inline]
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// shutdown server
    #[inline]
    pub fn shutdown(&self) {
        assert!(self.running());
        if let Err(err) = self.shutdown.send(1) {
            tracing::error!("server.shutdown failed: {:?}", err);
        }
    }

    /// wait for server release
    pub async fn wait(&self) {
        while self.max_connections > self.limit_connections.available_permits() || self.running() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

impl<T> Default for Server<T>
where
    T: IServerEvent,
{
    fn default() -> Self {
        Self::new(
            g::DEFAULT_HOST,
            g::DEFAULT_MAX_CONNECTIONS,
            g::DEFAULT_READ_TIMEOUT,
        )
    }
}

/// # Conn<U>
///
/// network connections
///
/// # Generic U
///
/// User custome data.
pub struct Conn<U: Default + Send + Sync> {
    #[cfg(unix)]
    sockfd: i32, // unix raw socket
    #[cfg(windows)]
    sockfd: RawSocket, // windows raw socket
    idempoetnt: u32, // current idempotent
    send_seq: u32,
    recv_seq: u32,
    remote: SocketAddr,
    local: SocketAddr,
    wbuf_sender: broadcast::Sender<Response>, // write channel
    shutdown_sender: broadcast::Sender<u8>,   // shutdown channel
    rbuf: BytesMut,                           // read buffer
    user_data: Option<U>,                     // user data
}

impl<U: Default + Send + Sync> Conn<U> {
    /// # Conn<U>::new
    ///
    /// make a default `Conn`
    ///
    /// this methods for internal use.
    fn new() -> Self {
        let (wch_sender, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);
        let (shutdown_sender, _) = broadcast::channel(1);
        Self {
            sockfd: 0,
            idempoetnt: 0,
            send_seq: 0,
            recv_seq: 0,
            remote: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            local: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            wbuf_sender: wch_sender,
            shutdown_sender,
            rbuf: BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
            user_data: None,
        }
    }

    /// # Conn<U>::pool
    /// make Conn<U> object pool
    ///
    /// @PS: RUST not support static variable with generic paramemters
    ///
    ///    * static POOL<T>: LinearObjectPool<Conn<T>> = LinearObjectPool::new(...);
    ///
    ///    * static POOL: LinearObjectPool<Conn<T>> = LinearObjectPool::new(...);
    ///
    /// # Example
    ///
    /// ```
    /// lazy_static::lazy_static! {
    ///     static ref CONN_POOL: lockfree_object_pool::LinearObjectPool<tg::nw::Conn<()>> = tg::nw::Conn::<()>::pool();
    /// }
    /// ```
    pub fn pool() -> LinearObjectPool<Self> {
        LinearObjectPool::new(
            || Self::new(),
            |v| {
                v.reset();
            },
        )
    }

    /// Conn active by TcpStream
    fn acitve(
        &mut self,
        stream: &TcpStream,
    ) -> (
        broadcast::Sender<Response>,
        broadcast::Receiver<Response>,
        broadcast::Receiver<u8>,
    ) {
        stream.set_nodelay(true).unwrap();

        #[cfg(unix)]
        {
            self.sockfd = stream.as_raw_fd();
        }

        #[cfg(windows)]
        {
            self.sockfd = stream.as_raw_socket();
        }

        self.remote = stream.peer_addr().unwrap();
        self.local = stream.local_addr().unwrap();

        (
            self.wbuf_sender.clone(),
            self.wbuf_sender.subscribe(),
            self.shutdown_sender.subscribe(),
        )
    }

    /// reset Conn<U>
    #[inline]
    fn reset(&mut self) {
        self.sockfd = 0;
        self.idempoetnt = 0;
        self.send_seq = 0;
        self.recv_seq = 0;
        self.user_data = None;
        self.rbuf.resize(g::DEFAULT_BUF_SIZE, 0);
        unsafe {
            self.rbuf.set_len(0);
        }
    }

    /// check read buffer.
    ///
    /// if read buffer size is less than pack::Package::HEAD_SIZE, resize to g::DEFAULT_BUF_SIZE.
    ///
    /// for internal to use.
    #[inline]
    fn check_rbuf(&self) {
        let n = self.rbuf.len();
        if n < pack::Package::HEAD_SIZE {
            unsafe {
                let rbuf = &mut *(&self.rbuf as *const BytesMut as *mut BytesMut);
                rbuf.resize(g::DEFAULT_BUF_SIZE, 0);
                rbuf.set_len(n)
            };
        }
    }

    /// get raw socket
    #[inline]
    #[cfg(unix)]
    pub fn sockfd(&self) -> i32 {
        self.sockfd
    }

    /// get raw socket
    #[inline]
    #[cfg(windows)]
    pub fn sockfd(&self) -> RawSocket {
        self.sockfd
    }

    /// get remote address
    #[inline]
    pub fn remote(&self) -> &SocketAddr {
        &self.remote
    }

    /// get local address
    #[inline]
    pub fn local(&self) -> &SocketAddr {
        &self.local
    }

    /// close the Conn<U>'s conenction
    #[inline]
    pub fn shutdown(&self) {
        debug_assert!(self.sockfd > 0);
        self.shutdown_sender.send(1).unwrap();
    }

    /// get connection's read buffer as mutable.
    #[inline]
    fn rbuf_mut(&self) -> &mut BytesMut {
        unsafe { &mut *(&self.rbuf as *const BytesMut as *mut BytesMut) }
    }

    /// get recv seqenece.
    #[inline]
    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    #[inline]
    pub fn recv_seq_incre(&self) {
        unsafe {
            let p = &self.recv_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// get send seqenece.
    #[inline]
    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    #[inline]
    pub fn send_seq_incre(&self) {
        unsafe {
            let p = &self.send_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// set user data
    #[inline]
    pub fn set_user_data(&mut self, user_data: U) {
        self.user_data = Some(user_data)
    }

    /// get user data
    #[inline]
    pub fn user_data(&self) -> Option<&U> {
        self.user_data.as_ref()
    }

    /// send response to remote.
    #[inline]
    pub fn send(&self, data: Response) -> g::Result<()> {
        if self.sockfd == 0 {
            return Err(g::Err::ConnInvalid);
        }

        if let Err(_) = self.wbuf_sender.send(data) {
            return Err(g::Err::TcpWriteFailed(
                "wbuf_sender.send failed".to_string(),
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod nw_test {
    use super::Conn;

    #[test]
    fn conn_info() {
        println!(
            "* --------- Conn INFO BEGIN ---------\n\
            * Conn<()> size: {}\n\
            * --------- Conn INFO END ---------\n",
            std::mem::size_of::<Conn<()>>()
        );
    }
}

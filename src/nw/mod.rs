pub mod pack;
pub mod tcp;
pub mod tools;

use crate::g;
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

// ---------------------------------------------- nw::IServerEvent ----------------------------------------------
//
//
/// # IServerEvent
///
/// 服务端网络事件
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
/// impl tg::nw::IServerEvent for DemoEvent {
///     // make IServerEvent::U => ().
///     type U = ();
///
///     async fn on_process(&self, conn: &tg::nw::ConnPtr<()>, req: &tg::nw::pack::Package) -> tg::g::Result<Option<tg::nw::pack::PackBuf>> {
///         println!("{:?} => {:?}", conn.remote(), req.data());
///         Ok(None)
///     }
/// }
/// ```
#[async_trait]
pub trait IServerEvent: Default + Send + Sync + Clone + 'static {
    type U: Sync + Send + Default;

    /// 会话端错误事件
    ///
    /// # 触发
    ///
    /// 在会话端会读写出现错误时触发.
    async fn on_error(&self, conn: &ConnPtr<Self::U>, err: g::Err) {
        tracing::error!("[{} - {:?}] => {err}", conn.sockfd, conn.remote);
    }

    /// 会话端连接成功事件
    ///
    /// # 触发
    ///
    /// 在会话端连接连接成功后触发.
    async fn on_connected(&self, conn: &ConnPtr<Self::U>) -> g::Result<()> {
        tracing::debug!("[{} - {:?}] has connected", conn.sockfd, conn.remote);
        Ok(())
    }

    /// 会话端连接断开事件
    ///
    /// # 触发
    ///
    /// 在会话端连接断开后触发.
    async fn on_disconnected(&self, conn: &ConnPtr<Self::U>) {
        tracing::debug!("[{} - {:?}] has disconnected", conn.sockfd, conn.remote);
    }

    /// 会话端消息事件
    ///
    /// # 触发
    ///
    /// 在会话端成功解析消息后触发.
    async fn on_process(
        &self,
        conn: &ConnPtr<Self::U>,
        req: &pack::Package,
    ) -> g::Result<Option<pack::PackBuf>>;

    /// 服务端运行事件
    ///
    /// # 触发
    ///
    /// 在服务端开启监听之前触发.
    async fn on_running(&self, server: &Arc<Server<Self>>) {
        tracing::debug!(
            "server[ HOST:({}) | MAX:({}) | TIMOUT:({}) ] is running...",
            server.host,
            server.max_connections,
            server.timeout
        );
    }

    /// 服务端停止事件
    ///
    /// # 触发
    ///
    /// 在服务端退出监听轮循后触发.
    async fn on_stopped(&self, server: &Arc<Server<Self>>) {
        tracing::debug!("server[ HOST:({}) ] has stopped...!!!", server.host);
    }
}

// ---------------------------------------------- nw::Server<T> ----------------------------------------------
//
//
/// # Server<T>
///
/// 服务端属性
///
/// # 泛型: T
///
/// IServerEvent 接口实现.
pub struct Server<T> {
    event: T,
    host: &'static str,                 // 监听地址
    max_connections: usize,             // 最大连接数
    timeout: u64,                       // 会话端读超时
    running: AtomicBool,                // 运行状态
    limit_connections: Arc<Semaphore>,  // 连接退制信号量
    shutdown_tx: broadcast::Sender<u8>, // 关闭管道
}

impl<T: IServerEvent> Server<T> {
    /// # Server<T>::new
    ///
    /// Server<T> 工厂函数
    ///
    /// # 入参
    ///
    /// `host` 监听地址.
    ///
    /// `max_connections` 最大连接数.
    ///
    /// `timeout` 会话端读超时(单位秒).
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
    /// impl tg::nw::IServerEvent for DemoEvent {
    ///     type U = ();
    ///
    ///     async fn on_process(&self, conn: &tg::nw::ConnPtr<()>, req: &tg::nw::pack::Package) -> tg::g::Result<Option<tg::nw::pack::PackBuf>> {
    ///         println!("{:?} => {:?}", conn.remote(), req.data());
    ///         Ok(None)
    ///     }
    /// }
    ///
    /// let server = Server::<DemoEvent>::new("0.0.0.0:6688", 100, 60);
    /// assert_eq!(server.max_connections(), 100);
    /// assert_eq!(server.host(), "0.0.0.0:6688");
    /// ```
    pub fn new(host: &'static str, max_connections: usize, timeout: u64) -> Self {
        let (shutdown, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);

        Self {
            host,
            max_connections,
            timeout,
            limit_connections: Arc::new(Semaphore::new(max_connections)),
            event: T::default(),
            shutdown_tx: shutdown,
            running: AtomicBool::new(false),
        }
    }

    /// # Server<T>::new_ptr
    ///
    /// 创建 Arc<Server<T>> 实例
    ///
    /// # 入参
    /// 
    /// 参考 [Server<T>::new]
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
    /// impl tg::nw::IServerEvent for DemoEvent {
    ///     type U = ();
    ///
    ///     async fn on_process(&self, conn: &tg::nw::ConnPtr<()>, req: &tg::nw::pack::Package) -> tg::g::Result<Option<tg::nw::pack::PackBuf>> {
    ///         println!("{:?} => {:?}", conn.remote(), req.data());
    ///         Ok(None)
    ///     }
    /// }
    ///
    /// let server = Server::<DemoEvent>::new_ptr("0.0.0.0:6688", 100, 60);
    /// assert_eq!(server.max_connections(), 100);
    /// assert_eq!(server.host(), "0.0.0.0:6688");
    /// ```
    pub fn new_ptr(host: &'static str, max_connections: usize, timeout: u64) -> Arc<Self> {
        Arc::new(Self::new(host, max_connections, timeout))
    }

    /// 监听地址
    #[inline]
    pub fn host(&self) -> &'static str {
        self.host
    }

    /// 最大连接数
    #[inline]
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// 当前连接数
    /// 
    /// # Notes
    /// 
    /// 当前连接数并不准确, 仅作参考
    #[inline]
    pub fn current_connections(&self) -> usize {
        self.max_connections - self.limit_connections.available_permits()
    }

    /// 读超时
    #[inline]
    pub fn timeout(&self) -> u64 {
        self.timeout
    }

    /// 判断服务端是否处于监听(运行)状态.
    #[inline]
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 关闭服务监听
    #[inline]
    pub fn shutdown(&self) {
        debug_assert!(self.running());
        if let Err(err) = self.shutdown_tx.send(1) {
            tracing::error!("server.shutdown failed: {err}");
        }
    }

    /// 等待服务端关闭
    pub async fn wait(&self) {
        while self.max_connections > self.limit_connections.available_permits() || self.running() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

// ---------------------------------------------- nw::Conn<U> ----------------------------------------------
//
//
/// # ConnPtr<T>
/// 
/// 会话端指针
pub type ConnPtr<T> = Arc<LinearReusable<'static, Conn<T>>>;
pub type ConnPool<T> = LinearObjectPool<Conn<T>>;

/// # Conn<U>
///
/// 网络交互中的会话端
///
/// # 泛型: U
///
/// 用户自定义类型
pub struct Conn<U: Default + Send + Sync> {
    #[cfg(unix)]
    sockfd: i32, // unix raw socket
    #[cfg(windows)]
    sockfd: RawSocket, // windows raw socket
    idempotent: u32, // 当前幂等, 用来确认消息是否过期
    send_seq: u32,
    recv_seq: u32,
    remote: SocketAddr,
    local: SocketAddr,
    wbuf_sender: broadcast::Sender<pack::PackBuf>, // socket 发送管道
    shutdown_sender: broadcast::Sender<u8>,         // 会话关闭管道
    rbuf: BytesMut,                                 // read buffer
    user_data: Option<U>,                           // user data
}

impl<U: Default + Send + Sync> Conn<U> {
    /// # Conn<U>::new
    ///
    /// 创建默认的会话端实例, 该函数由框架内部调用, 用于 对象池的初始化
    fn new() -> Self {
        let (wbuf_sender, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);
        let (shutdown_sender, _) = broadcast::channel(1);
        Self {
            sockfd: 0,
            idempotent: 0,
            send_seq: 0,
            recv_seq: 0,
            remote: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            local: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            wbuf_sender,
            shutdown_sender,
            rbuf: BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
            user_data: None,
        }
    }

    /// # Conn<U>::pool
    /// 
    /// 创建 Conn<U> 对象池
    ///
    /// @PS: RUST 不支持静态变量代有泛型参数
    ///
    ///    * static POOL<T>: LinearObjectPool<Conn<T>> = LinearObjectPool::new(...);
    ///
    ///    * static POOL: LinearObjectPool<Conn<T>> = LinearObjectPool::new(...);
    ///
    /// # Example
    ///
    /// ```
    /// lazy_static::lazy_static! {
    ///     static ref CONN_POOL: tg::nw::ConnPool<()> = tg::nw::Conn::<()>::pool();
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

    /// # Conn<U>.setup
    /// 
    /// 激活会话端
    /// 
    /// # Returns
    /// 
    /// (`wbuf_tx`: io发送管道 sender, `wbuf_rx`: io发送管道 receiver, `shutdown_rx`: 会话关闭管道 receiver)
    /// 
    /// # Notes
    /// 
    /// 当会话端从对象池中被取出来时, 处于未激活状态, 未激活的会话端不能正常使用.
    fn setup(
        &mut self,
        stream: &TcpStream,
    ) -> (
        broadcast::Sender<pack::PackBuf>,   // io发送管道 sender
        broadcast::Receiver<pack::PackBuf>, // io发送管道 receiver
        broadcast::Receiver<u8>,            // 会话关闭管道 receiver
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

    /// # Conn<U>.reset
    ///
    /// 重置会话端, 使其处于未激活状态
    #[inline]
    fn reset(&mut self) {
        self.sockfd = 0;
        self.idempotent = 0;
        self.send_seq = 0;
        self.recv_seq = 0;
        self.user_data = None;
        self.rbuf.resize(g::DEFAULT_BUF_SIZE, 0);
        unsafe { self.rbuf.set_len(0); }
    }

    /// # Conn<U>.check_rbuf
    ///
    /// 检查读缓冲区, 当读缓冲区大小不够消息头长度时, 得置为默认长度
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

    /// 获取原始套接字
    #[inline]
    #[cfg(unix)]
    pub fn sockfd(&self) -> i32 {
        self.sockfd
    }

    /// 获取原始套接字
    #[inline]
    #[cfg(windows)]
    pub fn sockfd(&self) -> RawSocket {
        self.sockfd
    }

    /// 获取对端地址
    #[inline]
    pub fn remote(&self) -> &SocketAddr {
        &self.remote
    }

    /// 获取本端地址
    #[inline]
    pub fn local(&self) -> &SocketAddr {
        &self.local
    }

    /// 关闭会话端
    #[inline]
    pub fn shutdown(&self) {
        debug_assert!(self.sockfd > 0);
        self.shutdown_sender.send(1).unwrap();
    }

    /// 获取读缓冲区
    #[inline]
    fn rbuf_mut(&self) -> &mut BytesMut {
        unsafe { &mut *(&self.rbuf as *const BytesMut as *mut BytesMut) }
    }

    /// 获取接收序列
    #[inline]
    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    /// 接收序列递增
    #[inline]
    fn recv_seq_incr(&self) {
        unsafe {
            let p = &self.recv_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// 获取发送序列
    #[inline]
    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    // 发送序列递增
    #[inline]
    fn send_seq_incr(&self) {
        unsafe {
            let p = &self.send_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// 设置用户自定义数据
    #[inline]
    pub fn set_user_data(&mut self, user_data: U) {
        self.user_data = Some(user_data)
    }

    /// 获取用户自定义数据
    #[inline]
    pub fn user_data(&self) -> Option<&U> {
        self.user_data.as_ref()
    }

    /// 发送消息
    #[inline]
    pub fn send(&self, data: pack::PackBuf) -> g::Result<()> {
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

// ---------------------------------------------- IClientEvent ----------------------------------------------
//
//
/// # IClientEvent
/// 
/// 客户端网络事件
#[async_trait]
pub trait IClientEvent: Sync + Clone + Default + 'static {
    // 用户自定义数据
    type U: Default + Send + Sync;

    /// # on_connected
    /// 
    /// 客户端连接成功事件, 当成功与服务端连接后触发.
    async fn on_connected(&self, cli: &Client<Self::U>) -> g::Result<()> {
        tracing::debug!("{:?} => {:?} has connected", cli.local, cli.remote);
        Ok(())
    }

    /// # on_disconnected
    /// 
    /// 客户端断开连接事件, 当客户端与服务端连接断开后触发.
    fn on_disconnected(&self, cli: &Client<Self::U>) {
        tracing::debug!("{:?} => {:?} has disconnected", cli.local, cli.remote);
    }

    /// # on_error
    /// 
    /// 客户端错误事件, 当客户端产生读写错误后触发.
    async fn on_error(&self, cli: &Client<Self::U>, err: g::Err) {
        tracing::error!("{:?} => {:?}", cli.local, err);
    }

    /// # on_process
    /// 
    /// 客户端消息事件, 当客户端成功解析消息包后触发.
    async fn on_process(
        &self,
        cli: &Client<Self::U>,
        req: &pack::Package,
    ) -> g::Result<Option<pack::PackBuf>>;
}

// ---------------------------------------------- Client<T> ----------------------------------------------
//
//
pub struct Client<U: Default + Send + Sync> {
    #[cfg(unix)]
    sockfd: i32, // 原始套接字
    recv_seq: u32,
    send_seq: u32,
    idempotent: u32,
    #[cfg(windows)]
    sockfd: u64, // 原始套接字
    timeout: u64, // 读超时
    remote: SocketAddr,
    local: SocketAddr, 
    rbuf: BytesMut, // 读缓冲区
    wbuf_consumer: async_channel::Receiver<pack::PackBuf>, // 消费者写管道, 多个Client会抢占从该管道获取需要发送的消息, 达到负载均衡的目的
    shutdown_rx: broadcast::Receiver<u8>, // 客户端关闭管道
    wbuf_tx: broadcast::Sender<pack::PackBuf>, // io 发送管道
    user_data: Option<U>, // 用户数据
}

impl<U: Default + Send + Sync> Client<U> {
    /// # Client<U>::new
    /// 
    /// 创建新的 Client<U> 对象
    /// 
    /// # 入参
    /// 
    /// `timeout` 读超时
    /// 
    /// `wbuf_consumer` 消费者写管道
    /// 
    /// `shutdown_rx` 客户端关闭管道
    /// 
    /// `user_data` 用户数据
    fn new(
        timeout: u64,
        wbuf_consumer: async_channel::Receiver<pack::PackBuf>,
        shutdown_rx: broadcast::Receiver<u8>,
        user_data: Option<U>,
    ) -> Self {
        let (wbuf_tx, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);

        Self {
            sockfd: 0,
            send_seq: 0,
            recv_seq: 0,
            idempotent: 0,
            timeout,
            remote: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            local: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            rbuf: BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
            wbuf_consumer,
            shutdown_rx,
            wbuf_tx,
            user_data,
        }
    }

    /// # Client<U>::new
    /// 
    /// 创建新的 Client<U> 对象
    /// 
    /// # 入参
    /// 
    /// `timeout` 读超时
    /// 
    /// `wbuf_consumer` 消费者写管道
    /// 
    /// `shutdown_rx` 客户端关闭管道
    /// 
    /// `user_data` 用户数据
    /// 
    /// # Example
    /// 
    /// ```
    /// #[derive(Default, Clone, Copy)]
    /// struct DemoEvent;
    /// 
    /// #[async_trait::async_trait]
    /// impl tg::nw::IClientEvent for DemoEvent {
    ///     type U = ();
    /// 
    ///     async fn on_process(
    ///         &self,
    ///         cli: &tg::nw::Client<()>,
    ///         req: &tg::nw::pack::Package,
    ///     ) -> tg::g::Result<Option<tg::nw::pack::PackBuf>> {
    ///         tracing::debug!(
    ///             "from server[{:?}]: {} => {}",
    ///             cli.local(),
    ///             std::str::from_utf8(req.data()).unwrap(),
    ///             req.idempotent(),
    ///         );
    /// 
    ///         Ok(None)
    ///     }
    /// }
    /// 
    /// let (shutdown, _) = tokio::sync::broadcast::channel(1);
    /// let (producer, consumer) = async_channel::bounded(tg::g::DEFAULT_CHAN_SIZE);
    /// let cli = tg::nw::Client::<()>::new_arc(0, consumer, shutdown.subscribe(), None);
    /// // ...
    /// ```
    pub fn new_arc(        
        timeout: u64,
        wbuf_consumer: async_channel::Receiver<pack::PackBuf>,
        shutdown_rx: broadcast::Receiver<u8>,
        user_data: Option<U>,
    ) -> Arc<Self> {
        Arc::new(Self::new(timeout, wbuf_consumer, shutdown_rx, user_data))
    }

    #[cfg(unix)]
    /// 获取原始套接字
    #[inline]
    pub fn sockfd(&self) -> i32 {
        self.sockfd
    }

    #[cfg(windows)]
    /// 获取原始套接字
    #[inline]
    pub fn sockfd(&self) -> u64 {
        self.sockfd
    }

    /// 获取接收序列
    #[inline]
    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    /// 接收序列递增
    #[inline]
    fn recv_seq_incr(&self) {
        unsafe {
            let p = &self.recv_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// 获取发送序列
    #[inline]
    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    /// 发送序列递增
    #[inline]
    fn send_seq_incr(&self) {
        unsafe {
            let p = &self.send_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// 获取幂等
    #[inline]
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    /// 设置幂等
    #[inline]
    fn set_idempotent(&self, idempotent: u32) {
        unsafe {
            let p = &self.idempotent as *const u32 as *mut u32;
            *p = idempotent;
        }
    }

    /// 获取超时
    #[inline]
    pub fn timeout(&self) -> u64 {
        self.timeout
    }

    /// 获取对端地址
    #[inline]
    pub fn remote(&self) -> &SocketAddr {
        &self.remote
    }

    /// 获取本端地址
    #[inline]
    pub fn local(&self) -> &SocketAddr {
        &self.local
    }

    /// 装载
    /// 
    /// 获取Client 的有效操作管道
    /// 
    /// # Returns
    /// 
    /// 1, 关闭管道 receiver
    /// 
    /// 2, 消息者管道 receiver
    /// 
    /// 3, io 管道 sender
    /// 
    /// 4, io 管道 receiver
    #[inline]
    fn setup(&self) -> (broadcast::Receiver<u8>, async_channel::Receiver<pack::PackBuf>, broadcast::Sender<pack::PackBuf>, broadcast::Receiver<pack::PackBuf>) {
        (self.shutdown_rx.resubscribe(), self.wbuf_consumer.clone(), self.wbuf_tx.clone(), self.wbuf_tx.subscribe())
    }

    /// 获取用户数据
    #[inline]
    pub fn user_data(&self) -> Option<&U> {
        self.user_data.as_ref()
    }

    /// 发消息给对端
    #[inline]
    pub fn send(&self, wbuf: pack::PackBuf) {
        if let Err(_) = self.wbuf_tx.send(wbuf) {
            tracing::debug!("wbuf_tx.send failed");
        }
    }

    /// 获取mutable 读缓冲区
    #[inline]
    fn rbuf_mut(&self) -> &mut BytesMut{
        unsafe{ &mut *(&self.rbuf as *const BytesMut as *mut BytesMut) }
    }

    /// 获取读缓冲区
    #[inline]
    fn rbuf(&self) -> &BytesMut {
        &self.rbuf
    }

    /// 检查读缓冲区
    /// 
    /// 如果缓冲区大小不足以容纳消息头, 则会重新分配一个足够大的缓冲区
    #[inline]
    pub fn check_rbuf(&self) {
        let n = self.rbuf.len();
        if n < pack::Package::HEAD_SIZE {
            unsafe {
                let rbuf = &mut *(&self.rbuf as *const BytesMut as *mut BytesMut);
                rbuf.resize(g::DEFAULT_BUF_SIZE, 0);
                rbuf.set_len(n)
            };
        }
    }
}

// ---------------------------------------------- UNIT TEST ----------------------------------------------
//
//
#[cfg(test)]
mod nw_test {
    use super::Conn;
    use super::Client;

    #[test]
    fn conn_info() {
        println!(
            "* --------- Conn INFO BEGIN ---------\n\
            * Conn<()> size: {}\n\
            * --------- Conn INFO END ---------\n",
            std::mem::size_of::<Conn<()>>()
        );
    }

    #[test]
    fn client_info() {
        println!(
            "* --------- Client INFO BEGIN ---------\n\
            * Client<()> size: {}\n\
            * --------- Client INFO END ---------\n",
            std::mem::size_of::<Client<()>>()
        );
    }
}

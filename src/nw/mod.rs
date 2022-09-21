/// -----------------------------------
/// nw 网络库
/// author: iegad
/// time:   2022-09-20
/// update_timeline:
/// | ---- time ---- | ---- editor ---- | ------------------- content -------------------
pub mod pack;
pub mod tcp;
pub mod tools;

use crate::{g, us::Ptr};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use lockfree_object_pool::LinearObjectPool;
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

/// # IEvent
///
/// 通用网络事件
///
/// 该特型内置一个泛型 U 参数, 代表用户自定义数据.
#[async_trait]
pub trait IEvent: Default + Send + Sync + Clone + 'static {
    type U: Sync + Send + Default;

    /// 连接套接字读错误事件
    async fn on_error(&self, conn: &Conn<Self::U>, err: g::Err) {
        tracing::error!("[{}|{:?}] => {:?}", conn.sockfd, conn.remote(), err);
    }

    /// 连接套接字连接成功事件
    ///
    /// 当连接套接字连接成功之后触发
    async fn on_connected(&self, conn: &Conn<Self::U>) -> g::Result<()> {
        tracing::debug!("[{}|{:?}] has connected", conn.sockfd, conn.remote());
        Ok(())
    }

    /// 连接套接字连接断开事件
    ///
    /// 当连接套接字连接断开之后, 资源释放之前触发
    async fn on_disconnected(&self, conn: &Conn<Self::U>) {
        tracing::debug!("[{}|{:?}] has disconnected", conn.sockfd, conn.remote());
    }

    /// 连接套接字 package 处理过程
    ///
    /// 连接套接字读到完整 Package后触发
    async fn on_process(
        &self,
        conn: &Conn<Self::U>,
        req: &pack::Package,
    ) -> g::Result<Option<Bytes>>;
}

/// # IServerEvent
///
/// 服务端网络事件, 该特形依赖 IEvent 通用网络事件. 即, IServerEvent实现比需同时实现 IEvent
#[async_trait]
pub trait IServerEvent: IEvent {
    /// 服务端启动事件, 当服务端初始化完成, 监听前触发
    async fn on_runing(&self, server: ServerPtr<Self>) {
        tracing::debug!(
            "server[HOST:{}|MAX:{}|TIMOUT:{}] is running...",
            server.host,
            server.max_connections,
            server.timeout
        );
    }

    /// 服务端监听关闭后触发
    async fn on_stopped(&self, server: ServerPtr<Self>) {
        tracing::debug!("server[HOST:{}] has stopped...!!!", server.host);
    }
}

/// # Server<T>
///
/// 服务端
pub struct Server<T> {
    event: T,                          // 事件句柄
    host: &'static str,                // 监听地址
    max_connections: usize,            // 最大连接数
    timeout: u64,                      // 客户端读超时
    running: AtomicBool,               // 运行状态
    limit_connections: Arc<Semaphore>, // 连接限制信号量
    shutdown: broadcast::Sender<u8>,   // 停止管道(发送端)
}

/// # ServerPtr<T>
/// ServerPtr<T> 是 Arc<Ptr<Server<T>>> 别名.
///
/// Ptr是让 Server<T> 的不可变引用具有修改特证.
pub type ServerPtr<T> = Arc<Ptr<Server<T>>>;

impl<T> Server<T>
where
    T: IServerEvent,
{
    /// 创建服务端
    pub fn new_ptr(host: &'static str, max_connections: usize, timeout: u64) -> ServerPtr<T> {
        Ptr::parse(Self::new(host, max_connections, timeout))
    }

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

    /// 监听地址
    #[inline(always)]
    pub fn host(&self) -> &'static str {
        self.host
    }

    /// 最大连接数
    #[inline(always)]
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// 当前连接数
    #[inline(always)]
    pub fn current_connections(&self) -> usize {
        self.max_connections - self.limit_connections.available_permits()
    }

    /// 客户端读超时
    #[inline(always)]
    pub fn timeout(&self) -> u64 {
        self.timeout
    }

    /// 运行状态
    #[inline(always)]
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 关闭服务
    #[inline(always)]
    pub fn shutdown(&self) {
        assert!(self.running());
        if let Err(err) = self.shutdown.send(1) {
            tracing::error!("server.shutdown failed: {:?}", err);
        }
    }

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
    /// Ptr<T> 泛型的 Default 约束
    fn default() -> Self {
        Self::new(
            g::DEFAULT_HOST,
            g::DEFAULT_MAX_CONNECTIONS,
            g::DEFAULT_READ_TIMEOUT,
        )
    }
}

/// # Conn
///
/// 连接端
#[repr(C)]
pub struct Conn<U: Default> {
    #[cfg(unix)]
    sockfd: i32, // 类unix 平台下的原始socket
    #[cfg(windows)]
    sockfd: RawSocket, // windows 平台下的原始socket
    idempoetnt: u32,                        // 最后幂等值
    send_seq: u32,                          // 发送序列
    recv_seq: u32,                          // 接收序列
    remote: SocketAddr,                     // 远端地址
    local: SocketAddr,                      // 本端地址
    wbuf_sender: broadcast::Sender<Bytes>,  // 发送管道
    shutdown_sender: broadcast::Sender<u8>, // 关闭管道
    rbuf: BytesMut,                         // 读缓冲区
    user_data: Option<U>,                   // 用户数据
}

impl<U: Default> Conn<U> {
    /// 创建默认连接端
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

    /// 创建默认连接端对象池
    ///
    /// @PS: 目前RUST不支持 静态全局变量中有泛型参数, 所以只能提供一个创建对象池的方法, 然后在业务实中创建静态对象池.
    ///
    /// 不支持的语法:
    ///
    ///    1, static POOL<T>: LinearObjectPool<Conn<T>> = LinearObjectPool::new(...);
    ///
    ///    2, static POOL: LinearObjectPool<Conn<T>> = LinearObjectPool::new(...);
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

    /// 通过stream 加载Conn的参数, 只有 load_from之后, Conn<U>对象才能变得有效
    fn load_from(&mut self, stream: &TcpStream) {
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
    }

    /// 重置 Conn<U>, 使用无效
    #[inline(always)]
    fn reset(&mut self) {
        self.sockfd = 0;
        self.idempoetnt = 0;
        self.send_seq = 0;
        self.recv_seq = 0;
        self.user_data = None;
    }

    /// 检测读缓冲区.
    ///
    /// 随着连接端不断的读到消息, 读缓冲区的会越来越小, 所以读缓冲区一旦小于 消息头大小时需要重新分配读缓冲区空间
    #[inline(always)]
    fn check_rbuf(&mut self) {
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
    #[cfg(windows)]
    pub fn sockfd(&self) -> RawSocket {
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
    pub fn wbuf_receiver(&self) -> broadcast::Receiver<Bytes> {
        self.wbuf_sender.subscribe()
    }

    #[inline(always)]
    pub fn wbuf_sender(&self) -> broadcast::Sender<Bytes> {
        self.wbuf_sender.clone()
    }

    #[inline(always)]
    pub fn shutdown_sender(&self) -> broadcast::Sender<u8> {
        self.shutdown_sender.clone()
    }

    #[inline(always)]
    pub fn shutdown_receiver(&self) -> broadcast::Receiver<u8> {
        self.shutdown_sender.subscribe()
    }

    #[inline(always)]
    pub fn shutdown(&self) {
        debug_assert!(self.sockfd > 0);
        self.shutdown_sender.send(1).unwrap();
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

    #[inline(always)]
    pub fn set_user_data(&mut self, user_data: U) {
        self.user_data = Some(user_data)
    }

    #[inline(always)]
    pub fn user_data(&self) -> Option<&U> {
        self.user_data.as_ref()
    }

    #[inline(always)]
    pub fn send(&self, data: Bytes) -> g::Result<()> {
        if self.sockfd == 0 {
            return Err(g::Err::ConnInvalid);
        }

        if let Err(err) = self.wbuf_sender.send(data) {
            return Err(g::Err::TcpWriteFailed(format!("{:?}", err)));
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

/// -----------------------------------
/// nw 网络库
/// author: iegad
/// time:   2022-09-20
/// update_timeline:
/// | ---- time ---- | ---- editor ---- | ------------------- content -------------------
pub mod pack;
pub mod tcp;
use crate::{g, us::Ptr};
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use lockfree_object_pool::LinearObjectPool;
#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::{AsRawSocket, RawSocket};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::{
    net::TcpStream,
    sync::{broadcast, Semaphore},
};

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
    pub fn new(host: &'static str, max_connections: usize, timeout: u64) -> ServerPtr<T> {
        let (shutdown, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);

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

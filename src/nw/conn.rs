#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::{AsRawSocket, RawSocket};
use std::{sync::Arc, net::{SocketAddr, SocketAddrV4, Ipv4Addr}};
use lockfree_object_pool::{LinearReusable, LinearObjectPool};
use tokio::{sync::broadcast, net::TcpStream};
use crate::g;
use super::pack;

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
    // block
    #[cfg(unix)]
    sockfd: i32, // unix raw socket
    #[cfg(windows)]
    sockfd: RawSocket, // windows raw socket
    idempotent: u32, // 当前幂等, 用来确认消息是否过期
    send_seq: u32,
    recv_seq: u32,
    remote: SocketAddr,
    local: SocketAddr,
    rbuf: [u8; g::DEFAULT_BUF_SIZE],               // read buffer
    user_data: Option<U>,                          // user data
    // contorller
    wbuf_sender: broadcast::Sender<pack::PackBuf>, // socket 发送管道
    shutdown_sender: broadcast::Sender<u8>,        // 会话关闭管道
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
            rbuf: [0u8; g::DEFAULT_BUF_SIZE],
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
    ///     static ref CONN_POOL: tg::nw::conn::ConnPool<()> = tg::nw::conn::Conn::<()>::pool();
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
    pub(crate) fn setup(
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
    pub(crate) fn reset(&mut self) {
        self.sockfd = 0;
        self.idempotent = 0;
        self.send_seq = 0;
        self.recv_seq = 0;
        self.user_data = None;
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
    pub(crate) fn rbuf_mut(&self) -> &mut [u8] {
        unsafe { &mut *(&self.rbuf as *const [u8] as *mut [u8]) }
    }

    #[inline]
    pub(crate) fn rbuf(&self) -> &[u8] {
        &self.rbuf
    }

    /// 获取接收序列
    #[inline]
    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    /// 接收序列递增
    #[inline]
    pub(crate) fn recv_seq_incr(&self) {
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
    pub(crate) fn send_seq_incr(&self) {
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
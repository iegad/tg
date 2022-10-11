#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::{AsRawSocket, RawSocket};
use std::{net::{SocketAddr, SocketAddrV4, Ipv4Addr}, sync::Arc};
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use tokio::{sync::broadcast, net::TcpStream};
use crate::g;
use super::{pack, Socket};

// ---------------------------------------------- nw::Conn<U> ----------------------------------------------
//
//
/// # ConnPtr<T>
/// 
/// 会话端指针
pub type ConnPtr<T> = Arc<Conn<T>>;
pub type ConnItem<T> = LinearReusable<'static, ConnPtr<T>>;
pub type ConnPool<T> = LinearObjectPool<ConnPtr<T>>;

/// # Conn<U>
///
/// 网络交互中的会话端
///
/// # 泛型: U
///
/// 用户自定义类型
pub struct Conn<U: Default + Send + Sync> {
    // block
    sockfd: Socket,
    idempotent: u32, // 当前幂等, 用来确认消息是否过期
    send_seq: u32,
    recv_seq: u32,
    remote: SocketAddr,
    local: SocketAddr,
    rbuf: Vec<u8>,               // read buffer
    user_data: Option<U>,                          // user data
    // contorller
    wbuf_sender: broadcast::Sender<pack::PackBuf>, // socket 发送管道
    shutdown_sender: broadcast::Sender<u8>,        // 会话关闭管道
}

impl<U: Default + Send + Sync + 'static> Conn<U> {
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
            rbuf: vec![0u8; g::DEFAULT_BUF_SIZE],
            user_data: None,
        }
    }

    pub(crate) fn new_arc() -> Arc<Self> {
        Arc::new(Self::new())
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
    pub fn pool() -> ConnPool<U> {
        LinearObjectPool::new(Self::new_arc, |v|v.reset())
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
        &self,
        stream: &TcpStream,
    ) -> (
        broadcast::Sender<pack::PackBuf>,   // io发送管道 sender
        broadcast::Receiver<pack::PackBuf>, // io发送管道 receiver
        broadcast::Receiver<u8>,            // 会话关闭管道 receiver
    ) {
        stream.set_nodelay(true).unwrap();

        unsafe {
            let v = &self.sockfd as *const Socket as *mut Socket;
            #[cfg(unix)]
            { *v = stream.as_raw_fd(); }
    
            #[cfg(windows)]
            { *v = stream.as_raw_socket(); }
    
            let remote = &self.remote as *const SocketAddr as *mut SocketAddr;
            let local = &self.local as *const SocketAddr as *mut SocketAddr;
            *remote = stream.peer_addr().unwrap();
            *local = stream.local_addr().unwrap();
        }

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
    #[allow(clippy::cast_ref_to_mut)]
    pub(crate) fn reset(&self) {
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);

            this.sockfd = 0;
            this.idempotent = 0;
            this.send_seq = 0;
            this.recv_seq = 0;
            this.user_data = None;
        }
    }

    /// 获取原始套接字
    #[cfg(unix)]
    #[inline(always)]
    pub fn sockfd(&self) -> i32 {
        self.sockfd
    }

    /// 获取原始套接字
    #[cfg(windows)]
    #[inline(always)]
    pub fn sockfd(&self) -> RawSocket {
        self.sockfd
    }

    /// 获取对端地址
    #[inline(always)]
    pub fn remote(&self) -> &SocketAddr {
        &self.remote
    }

    /// 获取本端地址
    #[inline(always)]
    pub fn local(&self) -> &SocketAddr {
        &self.local
    }

    /// 关闭会话端
    #[inline(always)]
    pub fn shutdown(&self) {
        assert!(self.sockfd > 0);
        self.shutdown_sender.send(1).unwrap();
    }

    /// 获取读缓冲区
    #[inline(always)]
    #[allow(clippy::cast_ref_to_mut)]
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn rbuf_mut(&self) -> &mut [u8] {
        unsafe { &mut *(self.rbuf.as_ref() as *const [u8] as *mut [u8]) }
    }

    #[inline(always)]
    pub(crate) fn rbuf(&self) -> &[u8] {
        &self.rbuf
    }

    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    /// 获取接收序列
    #[inline(always)]
    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    /// 接收序列递增
    #[inline(always)]
    pub(crate) fn recv_seq_incr(&self) {
        unsafe {
            let p = &self.recv_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// 获取发送序列
    #[inline(always)]
    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    // 发送序列递增
    #[inline(always)]
    pub(crate) fn send_seq_incr(&self) {
        unsafe {
            let p = &self.send_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// 设置用户自定义数据
    #[inline(always)]
    pub fn set_user_data(&mut self, user_data: U) {
        self.user_data = Some(user_data)
    }

    /// 获取用户自定义数据
    #[inline(always)]
    pub fn user_data(&self) -> Option<&U> {
        self.user_data.as_ref()
    }

    /// 发送消息
    #[inline]
    pub fn send(&self, data: pack::PackBuf) -> g::Result<()> {
        if self.sockfd == 0 {
            return Err(g::Err::ConnInvalid);
        }

        if self.wbuf_sender.send(data).is_err() {
            return Err(g::Err::TcpWriteFailed(
                "wbuf_sender.send failed".to_string(),
            ));
        }

        Ok(())
    }
}

impl<U: Default + Sync + Send> Drop for Conn<U> {
    fn drop(&mut self) {
        tracing::warn!("{} has released", self.sockfd);
    }
}
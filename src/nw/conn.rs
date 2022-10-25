#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::AsRawSocket;
use std::{net::SocketAddr, sync::Arc};
use futures::channel::mpsc;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use tokio::{sync::broadcast, net::tcp::OwnedReadHalf};
use crate::g;
use super::{packet, server, Socket};

// ---------------------------------------------- nw::Conn<U> ----------------------------------------------
//
//
/// # ConnPtr<T>
/// 
/// 会话端指针
pub type LinearItem<'a, T> = LinearReusable<'static, Conn<'a, T>>;
pub type Pool<'a, T> = LinearObjectPool<Conn<'a, T>>;
pub type Ptr<'a, T> = Arc<LinearItem<'a, T>>;

/// # Conn<U>
///
/// 网络交互中的会话端
///
/// # 泛型: U
///
/// 用户自定义类型
pub struct Conn<'a, U: Default + Send + Sync + 'static> {
    // block
    idempotent: u32,
    send_seq: u32,
    recv_seq: u32,
    reader: Option<&'a OwnedReadHalf>,
    rbuf: [u8; g::DEFAULT_BUF_SIZE],
    user_data: Option<U>,
    builder: packet::Builder,
    // contorller
    shutdown_tx: broadcast::Sender<u8>,        // 会话关闭管道
    tx: mpsc::Sender<server::Packet>,
    rx: mpsc::Receiver<server::Packet>
}

impl<'a, U: Default + Send + Sync + 'static> Conn<'a, U> {
    /// # Conn<U>::new
    ///
    /// 创建默认的会话端实例, 该函数由框架内部调用, 用于 对象池的初始化
    fn new() -> Self {
        let (shutdown_sender, _) = broadcast::channel(1);
        let (tx, rx) = mpsc::channel(g::DEFAULT_CHAN_SIZE);
        Self {
            idempotent: 0,
            send_seq: 0,
            recv_seq: 0,
            reader: None,
            rbuf: [0; g::DEFAULT_BUF_SIZE],
            shutdown_tx: shutdown_sender,
            user_data: None,
            builder: packet::Builder::new(),
            tx,
            rx,
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
    ///     static ref CONN_POOL: tg::nw::conn::Pool<'static, ()> = tg::nw::conn::Conn::<()>::pool();
    /// }
    /// ```
    pub fn pool() -> Pool<'a, U> {
        LinearObjectPool::new(Self::new, |v|v.reset())
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
    #[allow(clippy::cast_ref_to_mut)]
    pub(crate) fn load(
        &self,
        reader: &'a tokio::net::tcp::OwnedReadHalf
    ) -> broadcast::Receiver<u8> {
        reader.as_ref().set_nodelay(true).unwrap();
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);
            this.reader = Some(reader);
        }
        self.shutdown_tx.subscribe()
    }

    /// # Conn<U>.reset
    ///
    /// 重置会话端, 使其处于未激活状态
    #[inline]
    #[allow(clippy::cast_ref_to_mut)]
    pub(crate) fn reset(&self) {
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);

            this.idempotent = 0;
            this.send_seq = 0;
            this.recv_seq = 0;
            this.user_data = None;
            this.reader = None;
            this.builder.reset();
        }
    }

    /// 获取原始套接字
    #[inline(always)]
    pub fn sockfd(&self) -> Option<Socket> {
        #[cfg(windows)]
        { self.reader.map(|r|r.as_ref().as_raw_socket()) }

        #[cfg(unix)]
        { self.reader.map(|r|r.as_ref().as_raw_fd()) }   
    }

    /// 获取对端地址
    #[inline(always)]
    pub fn remote(&self) -> Option<SocketAddr> {
        self.reader.map(|r|r.as_ref().peer_addr().expect("peer_addr called failed"))
    }

    #[inline(always)]
    pub fn local(&self) -> Option<SocketAddr> {
        self.reader.map(|r|r.as_ref().local_addr().expect("local_addr called failed"))
    }

    /// 关闭会话端
    #[inline(always)]
    pub fn shutdown(&self) {
        assert!(self.reader.is_some());
        self.shutdown_tx.send(1).unwrap();
    }

    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    #[inline(always)]
    pub fn set_idempotent(&self, idempotent: u32) {
        unsafe {
            let p = &self.idempotent as *const u32 as *mut u32;
            *p = idempotent;
        }
    }

    #[allow(clippy::mut_from_ref)]
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub(crate) fn builder(&self) -> &mut packet::Builder {
        unsafe { &mut *(&self.builder as *const packet::Builder as *mut packet::Builder) }
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

    #[allow(clippy::mut_from_ref)]
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub fn rbuf_mut(&self) -> &mut [u8] {
        unsafe { &mut *(&self.rbuf as *const [u8] as *mut [u8]) }
    }

    #[inline(always)]
    pub fn rbuf(&self, n: usize) -> & [u8] {
        &self.rbuf[..n]
    }

    #[allow(clippy::mut_from_ref)]
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub(crate) fn rx(&self) -> &mut mpsc::Receiver<server::Packet> {
        unsafe {&mut *(&self.rx as *const mpsc::Receiver<server::Packet> as *mut mpsc::Receiver<server::Packet>) }
    }

    /// 发送消息
    #[allow(clippy::cast_ref_to_mut)]
    #[inline]
    pub fn send(&self, data: super::server::Packet) -> g::Result<()> {
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);
            if let Err(err) = this.tx.try_send(data) {
                return Err(g::Err::IOWriteFailed(format!("conn.send failed: {err}")));
            }
        }

        Ok(())
    }
}

impl<'a, U: Default + Sync + Send> Drop for Conn<'a, U> {
    fn drop(&mut self) {
        tracing::warn!("{:?} has released", self.sockfd());
    }
}

pub struct Group {
    host: String,
    count: usize,
    tx: async_channel::Sender<server::Packet>,
    rx: async_channel::Receiver<server::Packet>,
    shutdown: tokio::sync::broadcast::Sender<u8>,
}

impl Group {
    pub fn new(host: &str, count: usize) -> Self {
        let (tx, rx) = async_channel::bounded(g::DEFAULT_CHAN_SIZE);
        let (shutdown, _) = tokio::sync::broadcast::channel(1);

        Self {
            host: host.to_string(),
            count,
            tx,
            rx,
            shutdown,
        }
    }

    pub fn new_arc(host: &str, count: usize) -> Arc<Self> {
        Arc::new(Self::new(host, count))
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub async fn send(&self, pkt: server::Packet) -> g::Result<()> {
        if let Err(err) = self.tx.send(pkt).await {
            return Err(g::Err::PackSendFailed(format!("{err}")));
        }

        Ok(())
    }

    pub fn sender(&self) -> async_channel::Sender<server::Packet> {
        self.tx.clone()
    }

    pub fn shutdown(&self) {
        if let Err(err) = self.shutdown.send(1) {
            tracing::error!("{err}");
            assert!(false);
        }
    }

    pub fn receiver(&self) -> async_channel::Receiver<server::Packet> {
        self.rx.clone()
    }

    pub fn shutdown_rx(&self) -> tokio::sync::broadcast::Receiver<u8> {
        self.shutdown.subscribe()
    }

    pub fn shutdown_tx(&self) -> tokio::sync::broadcast::Sender<u8> {
        self.shutdown.clone()
    }
}
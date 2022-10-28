//! 异步客户端
//! 
//! 用于主动连接服务的客户端使用.
//! 
//! 客户端由块(block)属性和(control)属性组成.
//! 
//! Block 属性用来描述对象的状态属性.
//! 
//! Control 属性用来描述对象的行为属性.

#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::AsRawSocket;
use std::{net::SocketAddr, sync::Arc};
use async_trait::async_trait;
use tokio::{net::tcp::OwnedReadHalf, sync::broadcast};
use crate::g;
use super::{packet, server, Socket};

// ---------------------------------------------- 类型重定义 ----------------------------------------------

/// 客户端指针
pub type Ptr<'a, T> = Arc<Client<'a, T>>;

// ---------------------------------------------- IEvent ----------------------------------------------

/// 客户端网络事件
/// 
/// 泛型U: 用来描述客户端的用户类型.
///
/// # Example
///
/// ```
/// use async_trait::async_trait;
///
/// #[derive(Clone, Default)]
/// struct DemoEvent;
///
/// #[async_trait]
/// impl tg::nw::client::IEvent for DemoEvent {
///     // make IEvent::U => ().
///     type U = ();
///
///     async fn on_process(&self, conn: &tg::nw::client::Ptr<()>, req: tg::nw::server::Packet) -> tg::g::Result<()> {
///         println!("{:?} => {:?}", conn.remote(), req.data());
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait IEvent: Clone + Default + Send + Sync + 'static {
    type U: Default + Send + Sync + 'static;

    /// 客户端消息事件, 在会话端成功解析消息后触发.
    async fn on_process(&self, cli: &Ptr<Self::U>, pkt: server::Packet) -> g::Result<()>;

    /// 客户端错误事件, 在会话端会读写出现错误时触发.
    async fn on_error(&self, cli: &Ptr<Self::U>, err: g::Err) {
        tracing::error!("[{:?} - {:?}] => {err}", cli.sockfd(), cli.remote());
    }

    /// 客户端连接成功事件, 在会话端连接连接成功后触发.
    async fn on_connected(&self, cli: &Ptr<Self::U>) -> g::Result<()> {
        tracing::debug!("[{:?} - {:?}] has connected", cli.sockfd(), cli.remote());
        Ok(())
    }

    /// 客户端连接断开事件, 在会话端连接断开后触发.
    async fn on_disconnected(&self, cli: &Ptr<Self::U>) {
        tracing::debug!("[{:?} - {:?}] has disconnected", cli.sockfd(), cli.remote());
    }
}

// ---------------------------------------------- Client ----------------------------------------------

/// 网络客户端
/// 
/// 泛型U: 用来描述客户端的用户类型.
/// 
/// 由块(block)属性和控制(control)属性组成.
pub struct Client<'a, U: Default + Send + Sync + 'static> {
    // block
    idempotent: u32,
    send_seq: u32,
    recv_seq: u32,
    reader: Option<&'a OwnedReadHalf>,
    user_data: Option<U>,
    // contorller
    builder: packet::Builder,
    shutdown_tx: broadcast::Sender<u8>,         // 控制客户端关闭
    tx: async_channel::Sender<server::Packet>,  // 控制消息包的转发 -- 异步消息传递
    rx: async_channel::Receiver<server::Packet> // 控制消息包的接收 -- 异步消息接收
}

impl<'a, U: Default + Send + Sync + 'static> Client<'a, U> {

    /// 构造函数
    #[inline]
    fn new(shutdown_tx: broadcast::Sender<u8>) -> Self {
        let (tx, rx) = async_channel::bounded(g::DEFAULT_CHAN_SIZE);
        Self::with(tx, rx, shutdown_tx)
    }

    /// 构造函数
    #[inline]
    fn with(tx: async_channel::Sender<server::Packet>, rx: async_channel::Receiver<server::Packet>, shutdown_tx: broadcast::Sender<u8>) -> Self {
        Self { 
            idempotent: 0, 
            send_seq: 0, 
            recv_seq: 0, 
            reader: None, 
            user_data: None, 
            builder: packet::Builder::new(), 
            shutdown_tx, 
            tx, 
            rx,
        }
    }

    /// Arc 构造函数
    /// 
    /// # Example
    /// 
    /// ```
    /// let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    /// let cli = tg::nw::client::Client::<'static, ()>::new_arc(shutdown_tx);
    /// cli.reset();
    /// ```
    #[inline]
    pub fn new_arc(shutdown_tx: broadcast::Sender<u8>) -> Arc<Self> {
        Arc::new(Self::new(shutdown_tx))
    }

    /// Arc 构造函数
    /// 
    /// # Example
    /// 
    /// ```
    /// let (tx, rx) = async_channel::bounded(100);
    /// let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    /// let cli = tg::nw::client::Client::<'static, ()>::with_arc(tx, rx, shutdown_tx);
    /// cli.reset();
    /// ```
    #[inline]
    pub fn with_arc(tx: async_channel::Sender<server::Packet>, rx: async_channel::Receiver<server::Packet>, shutdown_tx: broadcast::Sender<u8>) -> Arc<Self> {
        Arc::new(Self::with(tx, rx, shutdown_tx))
    }

    /// 加载客户端, 使其变得有效.
    /// 
    /// 默认创建的客户端是无效的, 是还没有建立网络连接的, 当成功建立网络连接之后需要把连接的 OwnedReadHalf 加载到客户端中, 才能使其有效.
    /// 
    /// 返回值:
    /// 
    ///  * 关闭管道rx
    ///  * 消息接收管道rx
    #[allow(clippy::cast_ref_to_mut)]
    #[inline]
    pub(crate) fn load(&self, reader: &'a OwnedReadHalf) -> (
        tokio::sync::broadcast::Receiver<u8>, 
        async_channel::Receiver<server::Packet>
    ) {
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);
            this.reader = Some(reader);
        }
        (self.shutdown_tx.subscribe(), self.rx.clone())
    }

    /// 重置客户端, 当客户端无效时, 用来重置客户端 block属性.
    #[allow(clippy::cast_ref_to_mut)]
    #[inline]
    pub fn reset(&self) {
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);
            this.idempotent = 0;
            this.send_seq = 0;
            this.recv_seq = 0;
            this.reader = None;
            this.user_data = None;
            this.builder.reset();
        }
    }

    /// 获取幂等
    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    /// 设置幂等
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub(crate) fn set_idempotent(&self, idempotent: u32) {
        unsafe { *(&self.idempotent as *const u32 as *mut u32) = idempotent; }
    }

    /// 获取发送序列
    #[inline(always)]
    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    /// 递增发送序列
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub(crate) fn send_seq_incr(&self) {
        unsafe { *(&self.send_seq as *const u32 as *mut u32) += 1; }
    }

    /// 获取接收序列
    #[inline(always)]
    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    /// 递增接收序列
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub(crate) fn recv_seq_incr(&self) {
        unsafe { *(&self.recv_seq as *const u32 as *mut u32) += 1; }
    }

    /// 获取 连接描述符
    #[inline(always)]
    pub fn sockfd(&self) -> Option<Socket> {
        #[cfg(windows)]
        { self.reader.as_ref().map(|r|r.as_ref().as_raw_socket()) }
        #[cfg(unix)]
        { self.reader.map(|r| r.as_ref().as_raw_fd()) }
    }

    /// 获取本端地址
    #[inline(always)]
    pub fn local(&self) -> Option<SocketAddr> {
        self.reader.as_ref().map(|r|r.local_addr().expect("local_addr called failed"))
    }

    /// 获取远端地址
    #[inline(always)]
    pub fn remote(&self) -> Option<SocketAddr> {
        self.reader.as_ref().map(|r|r.peer_addr().expect("peer_addr called failed"))
    }

    /// 获取Packet builder
    #[allow(clippy::cast_ref_to_mut)]
    #[allow(clippy::mut_from_ref)]
    #[inline(always)]
    pub(crate) fn builder(&self) -> &mut packet::Builder {
        unsafe { &mut *(&self.builder as *const packet::Builder as *mut packet::Builder) }
    }

    /// 关闭连接
    #[inline]
    pub fn shutdown(&self) {
        if self.reader.is_some() && self.shutdown_tx.receiver_count() > 0 {
            if let Err(err) = self.shutdown_tx.send(1) {
                panic!("----- shutdown_tx.send failed: {err} -----");
            }
        }
    }

    /// 获取用户数据
    #[inline(always)]
    pub fn user_data(&self) -> Option<&U> {
        self.user_data.as_ref()
    }

    /// 发送消息
    #[inline(always)]
    pub fn send(&self, pkt: server::Packet) -> g::Result<()> {
        match self.tx.try_send(pkt) {
            Err(err) => Err(g::Err::IOWriteFailed(format!("{err}"))),
            Ok(()) => Ok(()),
        }
    }
}

// ----------------------------------------------- Group -----------------------------------------------
pub struct Group {
    count: usize,
    shutdown_tx: broadcast::Receiver<u8>,
}

// ----------------------------------------------- UT -----------------------------------------------

#[cfg(test)]
mod client_test {
    use tokio::{net::tcp::OwnedReadHalf, sync::broadcast};
    use crate::nw::{packet, server, client::Client};


    #[test]
    fn conn_info() {
        println!("> ------------------------------ > Client: {}", std::mem::size_of::<Client<()>>());
        println!(">>>>> Option<&OwnedReadHalf> Size: {}", std::mem::size_of::<Option<&OwnedReadHalf>>());
        println!(">>>>> packet::Builder Size: {}", std::mem::size_of::<packet::Builder>());
        println!(">>>>> broadcast::Sender<u8> Size: {}", std::mem::size_of::<broadcast::Sender<u8>>());
        println!(">>>>> async_channel::Sender<server::Packet> Size: {}", std::mem::size_of::<async_channel::Sender<server::Packet>>());
        println!(">>>>> async_channel::Receiver<server::Packet> Size: {}", std::mem::size_of::<async_channel::Receiver<server::Packet>>());
    }
}
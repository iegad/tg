use std::{net::{SocketAddr, SocketAddrV4, Ipv4Addr}, sync::Arc};
use async_trait::async_trait;
use tokio::{sync::broadcast, net::TcpStream};
use crate::g;
use super::{pack, Socket};
#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::AsRawSocket;


// ---------------------------------------------- IClientEvent ----------------------------------------------
//
//
/// # IClientEvent
/// 
/// 客户端网络事件
#[async_trait]
pub trait IEvent: Sync + Clone + Default + 'static {
    // 用户自定义数据
    type U: Default + Send + Sync;

    /// # on_connected
    /// 
    /// 客户端连接成功事件, 当成功与服务端连接后触发.
    fn on_connected(&self, cli: &Client<Self::U>) -> g::Result<()> {
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
    // block
    sockfd: Socket,       // 原始套接字
    recv_seq: u32,        // 接收序列
    send_seq: u32,        // 发送序列
    idempotent: u32,      // 幂等
    timeout: u64,         // 读超时
    remote: SocketAddr,   // 对端地址
    local: SocketAddr,    // 本端地址
    rbuf: Vec<u8>,        // 读缓冲区
    user_data: Option<U>, // 用户数据
    // controller
    wbuf_consumer: async_channel::Receiver<pack::PackBuf>, // 消费者写管道
    shutdown_tx: broadcast::Sender<u8>,                  // 客户端关闭管道
    wbuf_tx: broadcast::Sender<pack::PackBuf>,             // io 发送管道
}

impl<U: Default + Send + Sync> Client<U> {
    /// # Client<U>::new
    /// 
    /// 创建新的 Client<U> 对象
    /// 
    /// # 入参
    /// 
    /// `timeout`       读超时(单位秒)
    /// 
    /// `wbuf_consumer` 消费者写管道, 多个Client会抢占从该管道获取需要发送的消息, 达到负载均衡的目的
    /// 
    /// `user_data`     用户数据
    pub(crate) fn new(
        timeout: u64,
        wbuf_consumer: async_channel::Receiver<pack::PackBuf>,
        user_data: Option<U>,
    ) -> Self {
        let (wbuf_tx, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);
        let (shutdown_tx, _) = broadcast::channel(1);

        Self {
            sockfd: 0,
            send_seq: 0,
            recv_seq: 0,
            idempotent: 0,
            timeout,
            remote: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            local: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            rbuf: vec![0u8; g::DEFAULT_BUF_SIZE],
            wbuf_consumer,
            shutdown_tx,
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
    /// impl tg::nw::client::IEvent for DemoEvent {
    ///     type U = ();
    /// 
    ///     async fn on_process(
    ///         &self,
    ///         cli: &tg::nw::client::Client<()>,
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
    /// let (producer, consumer) = async_channel::bounded(tg::g::DEFAULT_CHAN_SIZE);
    /// let (cli, controller) = tg::nw::client::Client::<()>::new_pair(0, consumer, None);
    /// // ...
    /// ```
    pub fn new_pair(        
        timeout: u64,
        wbuf_consumer: async_channel::Receiver<pack::PackBuf>,
        user_data: Option<U>,
    ) -> (Arc<Self>, Arc<Self>) {
        let c = Arc::new(Self::new(timeout, wbuf_consumer, user_data));
        (c.clone(), c)
    }

    /// 装载
    /// 
    /// 获取Client 的有效操作管道
    /// 
    /// # Returns
    /// 
    /// 1, 关闭管道 receiver
    /// 
    /// 2, 消费者管道 receiver
    /// 
    /// 3, io 管道 sender
    /// 
    /// 4, io 管道 receiver
    #[allow(clippy::cast_ref_to_mut)]
    pub(crate) fn setup(&self, stream: &mut TcpStream) -> (
        broadcast::Receiver<u8>,                // 关闭管道
        async_channel::Receiver<pack::PackBuf>, // 负载发送管道
        broadcast::Sender<pack::PackBuf>,       // io 写管道
        broadcast::Receiver<pack::PackBuf>      // io 读管道
    ) {
        unsafe {
            let cli = &mut *(self as *const Client<U> as *mut Client<U>);
            cli.remote = stream.peer_addr().unwrap();
            cli.local = stream.local_addr().unwrap();
            #[cfg(windows)]
            {
                cli.sockfd = stream.as_raw_socket();
            }
            #[cfg(unix)]
            {
                cli.sockfd = stream.as_raw_fd();
            }
        }

        (self.shutdown_tx.subscribe(), self.wbuf_consumer.clone(), self.wbuf_tx.clone(), self.wbuf_tx.subscribe())
    }

    /// 获取原始套接字
    #[inline(always)]
    pub fn sockfd(&self) -> super::Socket {
        self.sockfd
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

    /// 发送序列递增
    #[inline(always)]
    pub(crate) fn send_seq_incr(&self) {
        unsafe {
            let p = &self.send_seq as *const u32 as *mut u32;
            *p += 1;
        }
    }

    /// 获取幂等
    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    /// 设置幂等
    #[inline(always)]
    pub(crate) fn set_idempotent(&self, idempotent: u32) {
        unsafe {
            let p = &self.idempotent as *const u32 as *mut u32;
            *p = idempotent;
        }
    }

    /// 获取超时
    #[inline(always)]
    pub fn timeout(&self) -> u64 {
        self.timeout
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

    /// 获取用户数据
    #[inline(always)]
    pub fn user_data(&self) -> Option<&U> {
        self.user_data.as_ref()
    }

    /// 获取mutable 读缓冲区
    #[inline(always)]
    #[allow(clippy::cast_ref_to_mut)]
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn rbuf_mut(&self) -> &mut [u8] {
        unsafe{ &mut *(self.rbuf.as_ref() as *const [u8] as *mut [u8]) }
    }

    /// 获取读缓冲区
    #[inline(always)]
    pub(crate) fn rbuf(&self) -> &[u8] {
        &self.rbuf
    }

    /// 关闭客户端
    pub fn shutdown(&self) {
        if self.shutdown_tx.receiver_count() > 0 && self.shutdown_tx.send(1).is_err() {
            tracing::error!("[TG] shutdown_tx.send failed");
        }
    }

    /// 发消息给对端
    #[inline]
    pub fn send(&self, wbuf: pack::PackBuf) {
        if self.wbuf_tx.receiver_count() > 0 && self.wbuf_tx.send(wbuf).is_err() {
            tracing::error!("[TG] wbuf_tx.send failed");
        }
    }
}
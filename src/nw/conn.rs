//! 网络会话对象. 
//! 
//! 该对象作为 服务端侧接收到的客户端连接抽象. 由块(block)属性 和 控制(control)属性组成.
//! 
//! Block 属性用来描述对象的状态属性.
//! 
//! Control 属性用来描述对象的行为属性.
//! 
//! 在使用时应尽量通过对象池来获取实例. 通过对象池获取的实例.
//! 
//! 在运行时该对象由于占用字节过多, 所以尽量在堆上分配该实例, 但是对象池并不会将对象分配到堆上, 这个还是需要开发者手动控制.

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

// ----------------------------------------------- 类型重定义 -----------------------------------------------

/// LinearItem<T> 对象池元素
pub type LinearItem<'a, T> = LinearReusable<'static, Conn<'a, T>>;
/// Conn 对象池
pub type Pool<'a, T> = LinearObjectPool<Conn<'a, T>>;
/// Conn 会话指针, 使用这种对象池数据是为了提高性能.
pub type Ptr<'a, T> = Arc<LinearItem<'a, T>>;

// ----------------------------------------------- Conn -----------------------------------------------

/// 网络交互中的会话端
///
/// 泛型U 为用户自定义类型
pub struct Conn<'a, U: Default + Send + Sync + 'static> {
    // block
    idempotent: u32,
    send_seq: u32,
    recv_seq: u32,
    reader: Option<&'a OwnedReadHalf>,
    rbuf: [u8; g::DEFAULT_BUF_SIZE],
    user_data: Option<U>,
    // contorller
    builder: packet::Builder,
    shutdown_tx: broadcast::Sender<u8>, // 控制会话的关闭
    tx: mpsc::Sender<server::Packet>,   // 控制消息包的转发 -- 异步消息传递
    rx: mpsc::Receiver<server::Packet>  // 控制消息包的接收 -- 异步消息接收
}

impl<'a, U: Default + Send + Sync + 'static> Conn<'a, U> {
    /// 创建默认的会话端实例, 该函数由框架内部调用, 用于 对象池的初始化
    fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let (tx, rx) = mpsc::channel(g::DEFAULT_CHAN_SIZE);
        Self {
            idempotent: 0,
            send_seq: 0,
            recv_seq: 0,
            reader: None,
            rbuf: [0; g::DEFAULT_BUF_SIZE],
            shutdown_tx,
            user_data: None,
            builder: packet::Builder::new(),
            tx,
            rx,
        }
    }

    /// 创建会话 对象池
    /// 
    /// 该函数的使用包含到了调用宏中, 所以在没有特殊条件下, 尽量不要显示调用.
    ///
    /// `PS: RUST 不支持静态变量代有泛型参数`
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

    /// 加载 tcp stream 到会话, 调用后会返回会话关闭管道 rx.
    /// 
    /// # Notes
    /// 
    /// 当会话端从对象池中被取出来时, 处于未激活状态, 未激活的会话端不能正常使用.
    #[allow(clippy::cast_ref_to_mut)]
    pub(crate) fn load(
        &self,
        reader: &'a tokio::net::tcp::OwnedReadHalf
    ) -> broadcast::Receiver<u8> {
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);
            this.reader = Some(reader);
        }
        self.shutdown_tx.subscribe()
    }

    /// 重置会话端, 使其处于未激活状态
    #[allow(clippy::cast_ref_to_mut)]
    #[inline]
    fn reset(&self) {
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
        if self.reader.is_some() && self.shutdown_tx.receiver_count() > 0 {
            if let Err(err) = self.shutdown_tx.send(1) {
                panic!("------ self.shutdown_tx.send(1) failed: {err} ------");
            }
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
    pub fn set_idempotent(&self, idempotent: u32) {
        unsafe { *(&self.idempotent as *const u32 as *mut u32) = idempotent; }
    }

    /// 获取 packet builder
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
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub(crate) fn recv_seq_incr(&self) {
        unsafe { *(&self.recv_seq as *const u32 as *mut u32) += 1; }
    }

    /// 获取发送序列
    #[inline(always)]
    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    /// 发送序列递增
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub(crate) fn send_seq_incr(&self) {
        unsafe { *(&self.send_seq as *const u32 as *mut u32) += 1; }
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

    /// 获取mutable 读缓冲区
    #[allow(clippy::mut_from_ref)]
    #[allow(clippy::cast_ref_to_mut)]
    #[inline(always)]
    pub(crate) fn rbuf_mut(&self) -> &mut [u8] {
        unsafe { &mut *(&self.rbuf as *const [u8] as *mut [u8]) }
    }

    /// 获取immutable 读缓冲区
    #[inline(always)]
    pub(crate) fn rbuf(&self, n: usize) -> &[u8] {
        &self.rbuf[..n]
    }

    /// 获取消息管道 rx
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

// ----------------------------------------------- UT -----------------------------------------------

#[cfg(test)]
mod conn_test {
    use futures::channel::mpsc;
    use tokio::{net::tcp::OwnedReadHalf, sync::broadcast};
    use crate::nw::{packet, server};
    use super::Conn;

    #[test]
    fn conn_info() {
        println!("> ------------------------------ > Conn: {}", std::mem::size_of::<Conn<()>>());
        println!(">>>>> Option<&OwnedReadHalf> Size: {}", std::mem::size_of::<Option<&OwnedReadHalf>>());
        println!(">>>>> packet::Builder Size: {}", std::mem::size_of::<packet::Builder>());
        println!(">>>>> broadcast::Sender<u8> Size: {}", std::mem::size_of::<broadcast::Sender<u8>>());
        println!(">>>>> mpsc::Sender<server::Packet> Size: {}", std::mem::size_of::<mpsc::Sender<server::Packet>>());
        println!(">>>>> mpsc::Receiver<server::Packet> Size: {}", std::mem::size_of::<mpsc::Receiver<server::Packet>>());
        println!(">>>>> Conn Size: {}", std::mem::size_of::<Conn<()>>());
    }
}
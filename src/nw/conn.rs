#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::AsRawSocket;
use std::{net::SocketAddr, sync::Arc};
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use tokio::{sync::broadcast};
use crate::g;

// ---------------------------------------------- nw::Conn<U> ----------------------------------------------
//
//
/// # ConnPtr<T>
/// 
/// 会话端指针
pub type ConnItem<'a, T> = LinearReusable<'static, Conn<'a, T>>;
pub type ConnPool<'a, T> = LinearObjectPool<Conn<'a, T>>;
pub type ConnPtr<'a, T> = Arc<ConnItem<'a, T>>;

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
    reader: Option<&'a tokio::net::tcp::OwnedReadHalf>,
    user_data: Option<U>,
    // contorller
    shutdown_sender: broadcast::Sender<u8>,        // 会话关闭管道
    tx: Option<futures::channel::mpsc::UnboundedSender<super::server::Pack>>,
}

impl<'a, U: Default + Send + Sync + 'static> Conn<'a, U> {
    /// # Conn<U>::new
    ///
    /// 创建默认的会话端实例, 该函数由框架内部调用, 用于 对象池的初始化
    fn new() -> Self {
        let (shutdown_sender, _) = broadcast::channel(1);
        Self {
            idempotent: 0,
            send_seq: 0,
            recv_seq: 0,
            reader: None,
            shutdown_sender,
            user_data: None,
            tx: None,
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
    ///     static ref CONN_POOL: tg::nw::conn::ConnPool<'static, ()> = tg::nw::conn::Conn::<()>::pool();
    /// }
    /// ```
    pub fn pool() -> ConnPool<'a, U> {
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
    pub(crate) fn setup(
        &self,
        reader: &'a tokio::net::tcp::OwnedReadHalf,
        tx: futures::channel::mpsc::UnboundedSender<super::server::Pack>
    ) -> broadcast::Receiver<u8> {
        reader.as_ref().set_nodelay(true).unwrap();
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);
            this.reader = Some(reader);
            this.tx = Some(tx);
        }
        self.shutdown_sender.subscribe()
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
        }
    }

    /// 获取原始套接字
    #[inline(always)]
    pub fn sockfd(&self) -> Option<super::Socket> {
        if let Some(r) = self.reader {
            #[cfg(windows)]
            { Some(r.as_ref().as_raw_socket())}
            #[cfg(unix)]
            { Some(r.as_ref().as_raw_fd()) }    
        } else {
            None
        }
    }

    /// 获取对端地址
    #[inline(always)]
    pub fn remote(&self) -> Option<SocketAddr> {
        if let Some(r) = self.reader {
            match r.as_ref().peer_addr() {
                Err(err) => { 
                    tracing::error!("get remote failed: {err}");
                    None
                }
                Ok(v) => Some(v),
            } 
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn local(&self) -> Option<SocketAddr> {
        if let Some(r) = self.reader {
            Some(r.as_ref().local_addr().unwrap())
        } else {
            None
        }
    }

    /// 关闭会话端
    #[inline(always)]
    pub fn shutdown(&self) {
        assert!(self.reader.is_some());
        self.shutdown_sender.send(1).unwrap();
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
    pub fn send(&self, data: super::server::Pack) -> g::Result<()> {
        if let None = self.tx {
            return Err(g::Err::ConnInvalid);
        }

        unsafe {
            let this = &mut *(self as *const Self as *mut Self);
            if let Err(err) = this.tx.as_mut().unwrap().unbounded_send(data) {
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
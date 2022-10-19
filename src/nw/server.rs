use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use async_trait::async_trait;
use lockfree_object_pool::LinearReusable;
use tokio::sync::{Semaphore, broadcast};
use crate::g;
use super::conn::ConnPtr;

// ---------------------------------------------- IServerEvent ----------------------------------------------
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
/// impl tg::nw::server::IEvent for DemoEvent {
///     // make IServerEvent::U => ().
///     type U = ();
///
///     async fn on_process(&self, conn: &tg::nw::conn::ConnPtr<()>, req: &tg::nw::pack::Package) -> tg::g::Result<Option<tg::nw::pack::LinearItem>> {
///         println!("{:?} => {:?}", conn.remote(), req.data());
///         Ok(None)
///     }
/// }
/// ```
#[async_trait]
pub trait IEvent: Default + Send + Sync + Clone + 'static {
    type U: Sync + Send + Default;

    /// 会话端错误事件
    ///
    /// # 触发
    ///
    /// 在会话端会读写出现错误时触发.
    async fn on_error(&self, conn: &ConnPtr<Self::U>, err: g::Err) {
        tracing::error!("[{:?} - {:?}] => {err}", conn.sockfd(), conn.remote());
    }

    /// 会话端连接成功事件
    ///
    /// # 触发
    ///
    /// 在会话端连接连接成功后触发.
    async fn on_connected(&self, conn: &ConnPtr<Self::U>) -> g::Result<()> {
        tracing::debug!("[{:?} - {:?}] has connected", conn.sockfd(), conn.remote());
        Ok(())
    }

    /// 会话端连接断开事件
    ///
    /// # 触发
    ///
    /// 在会话端连接断开后触发.
    async fn on_disconnected(&self, conn: &ConnPtr<Self::U>) {
        tracing::debug!("[{:?} - {:?}] has disconnected", conn.sockfd(), conn.remote());
    }

    /// 会话端消息事件
    ///
    /// # 触发
    ///
    /// 在会话端成功解析消息后触发.
    async fn on_process(
        &self,
        conn: &ConnPtr<Self::U>,
        req: LinearReusable<'static, Vec<u8>>,
    ) -> g::Result<()>;

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
        tracing::debug!("server[ HOST:({}) ] has shutdown", server.host);
    }
}

// ---------------------------------------------- Server<T> ----------------------------------------------
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
    // block
    pub(crate) host: &'static str,                 // 监听地址
    pub(crate) max_connections: usize,             // 最大连接数
    pub(crate) timeout: u64,                       // 会话端读超时
    pub(crate) running: AtomicBool,                // 运行状态
    // controller
    pub(crate) event: T,
    pub(crate) limit_connections: Arc<Semaphore>,  // 连接退制信号量
    pub(crate) shutdown_tx: broadcast::Sender<u8>, // 关闭管道
}


impl<T: IEvent> Server<T> {
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
    fn new(host: &'static str, max_connections: usize, timeout: u64) -> Self {
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
    /// use tg::nw::server::Server;
    /// use async_trait::async_trait;
    ///
    /// #[derive(Copy, Clone, Default)]
    /// struct DemoEvent;
    ///
    /// #[async_trait]
    /// impl tg::nw::server::IEvent for DemoEvent {
    ///     type U = ();
    ///
    ///     async fn on_process(&self, conn: &tg::nw::conn::ConnPtr<()>, req: &tg::nw::pack::Package) -> tg::g::Result<Option<tg::nw::pack::LinearItem>> {
    ///         println!("{:?} => {:?}", conn.remote(), req.data());
    ///         Ok(None)
    ///     }
    /// }
    ///
    /// let server = Server::<DemoEvent>::new_arc("0.0.0.0:6688", 100, 60);
    /// assert_eq!(server.max_connections(), 100);
    /// assert_eq!(server.host(), "0.0.0.0:6688");
    /// ```
    pub fn new_arc(host: &'static str, max_connections: usize, timeout: u64) -> Arc<Self> {
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
        tracing::debug!("server has stopped!!!");
    }
}
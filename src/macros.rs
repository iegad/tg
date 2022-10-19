
/// tcp_server_run 宏
/// 
/// 启动TCP服务. 用来简化服务端调用.
/// 
/// `host: &str` 服务监听地址
/// 
/// `max: usize` 最大连接数
/// 
/// `timout: u64` 读超时
/// 
/// `event: tg::nw::server::IEvent` 事件实现的类型
/// 
/// # Example
/// 
/// ```ignore
/// use async_trait::async_trait;
/// use tg::{nw::pack, utils};
/// 
/// #[derive(Clone, Copy, Default)]
/// struct DemoEvent;
/// 
/// #[async_trait]
/// impl tg::nw::server::IEvent for DemoEvent {
///     type U = ();
/// 
///     async fn on_process(
///         &self,
///         conn: &tg::nw::conn::ConnPtr<()>,
///         req: &pack::Package,
///     ) -> tg::g::Result<Option<pack::PackBuf>> {
///         Ok(None)
///     }
/// }
/// 
/// #[tokio::main]
/// async fn main() {
///     utils::init_log();
///     tg::tcp_server_run!("0.0.0.0:6688", 100, 0, DemoEvent);
/// }
/// ```
#[macro_export]
macro_rules! tcp_server_run {
    ($host:expr, $max:expr, $timout:expr, $event:ty) => {
        use tg::nw::{conn::{Conn, ConnPool}, server::{IEvent, Server}, tcp::server_run};

        type TConnPool = ConnPool<'static, <$event as IEvent>::U>;

        lazy_static::lazy_static! {
            static ref CONN_POOL: TConnPool = Conn::pool();
        }

        let server = Server::<$event>::new_arc($host, $max, $timout);
        let c = server.clone();
        tokio::spawn(async move {
            if let Err(err) = server_run(server, &CONN_POOL).await {
                tracing::error!("{err}");
            }
        });
        match tokio::signal::ctrl_c().await {
            Err(err) => tracing::error!("SIGINT error: {err}"),
            Ok(()) => c.shutdown(),
        }
        c.wait().await;
    };
}

#[macro_export]
macro_rules! make_wbuf {
    ($buf:expr) => {
        Some(std::sync::Arc::new($buf))
    };
}
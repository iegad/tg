#[macro_export]
macro_rules! tcp_server_run {
    ($host:expr, $max:expr, $timeout:expr, $event:ty) => {
        use tg::nw::{conn::{Conn, ConnPool}, server::{IEvent, Server}, tcp::server_run};

        type TConnPool = ConnPool<<$event as IEvent>::U>;

        lazy_static::lazy_static! {
            static ref CONN_POOL: TConnPool = Conn::pool();
        }

        let server = Server::<$event>::new_arc($host, $max, $timeout);
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
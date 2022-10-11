#[macro_export]
macro_rules! tcp_server_run {
    ($host:expr, $max:expr, $timeout:expr, $event:ty, $p:expr) => {
        let server = tg::nw::server::Server::<$event>::new_arc($host, $max, $timeout);
        let c = server.clone();
        tokio::spawn(async move {
            if let Err(err) = tg::nw::tcp::server_run(server, $p).await {
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
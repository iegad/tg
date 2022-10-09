#[macro_export]
macro_rules! tcp_server_run {
    ($s:expr, $c:expr, $p:expr) => {
        tokio::spawn(async move {
            if let Err(err) = tg::nw::tcp::server_run($s, $p).await {
                tracing::error!("{err}");
            }
        });
        match tokio::signal::ctrl_c().await {
            Err(err) => tracing::error!("SIGINT error: {err}"),
            Ok(()) => $c.shutdown(),
        }
    };
}
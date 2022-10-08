use async_trait::async_trait;
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use pack::WBUF_POOL;
use std::sync::Arc;
use tg::{nw::pack, utils};

#[derive(Clone, Default)]
struct EchoEvent;

#[async_trait]
impl tg::nw::server::IEvent for EchoEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &tg::nw::conn::ConnPtr<()>,
        req: &pack::Package,
    ) -> tg::g::Result<Option<pack::PackBuf>> {
        assert_eq!(req.idempotent(), conn.recv_seq());
        let mut rspbuf = WBUF_POOL.pull();
        tracing::debug!("{}", req);
        req.to_bytes(&mut rspbuf).unwrap();
        Ok(Some(Arc::new(rspbuf)))
    }

    async fn on_disconnected(&self, conn: &tg::nw::conn::ConnPtr<()>) {
        tracing::info!(
            "[{} - {:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
    }
}

lazy_static! {
    static ref CONN_POOL: LinearObjectPool<tg::nw::conn::Conn<()>> = tg::nw::conn::Conn::<()>::pool();
}

#[tokio::main]
async fn main() {
    utils::init_log();

    let (controller, server) = 
        tg::nw::server::Server::<EchoEvent>::new_pair("0.0.0.0:6688", 100, tg::g::DEFAULT_READ_TIMEOUT);

    tokio::spawn(async move {
        if let Err(err) = tg::nw::tcp::server_run(server, &CONN_POOL).await {
            println!("{err}");
        }
    });

    match tokio::signal::ctrl_c().await {
        Err(err) => tracing::error!("SIGINT error: {err}"),
        Ok(()) => controller.shutdown(),
    }

    controller.wait().await;
}

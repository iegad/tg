use async_trait::async_trait;
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use tg::{nw::pack, utils};

#[derive(Clone, Copy, Default)]
struct SimpleEvent;

#[async_trait]
impl tg::nw::IServerEvent for SimpleEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &tg::nw::ConnPtr<()>,
        req: &pack::Package,
    ) -> tg::g::Result<Option<pack::PackBuf>> {
        assert_eq!(req.idempotent(), conn.recv_seq());
        Ok(None)
    }

    async fn on_disconnected(&self, conn: &tg::nw::ConnPtr<()>) {
        tracing::debug!(
            "[{}|{:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
        assert_eq!(10000, conn.recv_seq());
    }
}

lazy_static! {
    static ref CONN_POOL: LinearObjectPool<tg::nw::Conn<()>> = tg::nw::Conn::<()>::pool();
}

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let controller =
        tg::nw::Server::<SimpleEvent>::new_ptr("0.0.0.0:6688", 100, tg::g::DEFAULT_READ_TIMEOUT);
    let server = controller.clone();

    tokio::spawn(async move {
        if let Err(err) = tg::nw::tcp::server_run(server, &CONN_POOL).await {
            println!("{:?}", err);
        }
    });

    match tokio::signal::ctrl_c().await {
        Err(err) => tracing::error!("SIGINT error: {:?}", err),
        Ok(()) => controller.shutdown(),
    }

    controller.wait().await;
}

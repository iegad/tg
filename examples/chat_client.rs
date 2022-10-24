use std::sync::Arc;

use async_trait::async_trait;
use tg::nw::packet::REQ_POOL;

#[derive(Clone, Default)]
struct EchoEvent;

#[async_trait]
impl tg::nw::server::IEvent for EchoEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &tg::nw::conn::Ptr<()>,
        req: tg::nw::server::Packet,
    ) -> tg::g::Result<()> {
        if req.idempotent() == 10000 {
            conn.shutdown();
        }
        Ok(())
    }

    #[allow(unused_variables)]
    async fn on_connected(&self, conn: &tg::nw::conn::Ptr<()>) -> tg::g::Result<()> {
        // tracing::info!("[{:?}] has connected", conn.remote());
        Ok(())
    }

    #[allow(unused_variables)]
    async fn on_disconnected(&self, conn: &tg::nw::conn::Ptr<()>) {
        tracing::info!("[{:?} - {:?}] has disconnected: {}", conn.sockfd(), conn.remote(), conn.send_seq());
    }
}

#[tokio::main]
async fn main() {
    tg::utils::init_log();

    lazy_static::lazy_static! {
        static ref CONN_POOL: tg::nw::conn::Pool<'static, ()> = tg::nw::conn::Conn::pool();
    }
    
    let conn = Arc::new(CONN_POOL.pull());
    let controller = conn.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

    let join = tokio::spawn(async move {
        tg::nw::tcp::connect::<EchoEvent>("127.0.0.1:6688", 0, shutdown_rx, conn).await.unwrap();
    });

    for i in 0..10000 {
        let mut p = REQ_POOL.pull();
        p.set(1, i + 1, "Hello world".as_bytes());
        controller.send(p).unwrap();
    }

    tracing::info!("send done");
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Err(err) => tracing::error!("SIGINT error: {err}"),
            Ok(()) => {
                shutdown_tx.send(1).unwrap();
            }
        }
    });

    join.await.unwrap();
}
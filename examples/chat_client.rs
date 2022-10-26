use async_trait::async_trait;
use tg::nw::{packet::REQ_POOL, client};


#[derive(Clone, Default)]
struct EchoEvent;

#[async_trait]
impl tg::nw::client::IEvent for EchoEvent {
    type U = ();

    async fn on_process(&self, cli: &tg::nw::client::Ptr<()>, req: tg::nw::server::Packet) -> tg::g::Result<()> {
        tracing::info!("[{:?}] {}", cli.remote(), *req);
        if req.idempotent() == 10000 {
            cli.shutdown();
        }
        Ok(())
    }

    #[allow(unused_variables)]
    async fn on_connected(&self, cli: &tg::nw::client::Ptr<()>) -> tg::g::Result<()> {
        // tracing::info!("[{:?}] has connected", conn.remote());
        Ok(())
    }

    #[allow(unused_variables)]
    async fn on_disconnected(&self, cli: &tg::nw::client::Ptr<()>) {
        tracing::info!("[{:?} - {:?}] has disconnected: {}", cli.sockfd(), cli.remote(), cli.send_seq());
    }
}

#[tokio::main]
async fn main() {
    tg::utils::init_log();

    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let cli = client::Client::<()>::new_arc(shutdown_tx);
    let controller = cli.clone();

    let j = tokio::spawn(async move {
        if let Err(err) = tg::nw::tcp::client_run::<EchoEvent>("127.0.0.1:6688", cli).await {
            tracing::error!("{err}");
        }
    });
    
    for i in 0..10000 {
        let mut pkt = REQ_POOL.pull();
        pkt.set(1, i + 1, "Hello world".as_bytes());

        if let Err(err) = controller.send(pkt) {
            tracing::error!("controller.send failed: {err}");
            break;
        }
    }
    
    j.await.unwrap();
    tracing::info!("done");
}
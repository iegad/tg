use async_trait::async_trait;
use tg::nw::{packet::REQ_POOL, client};


#[derive(Clone, Default)]
struct EchoEvent;

#[async_trait]
impl tg::nw::client::IEvent for EchoEvent {
    type U = ();

    async fn on_process(&self, cli: &tg::nw::client::Ptr<()>, req: tg::nw::server::Packet) -> tg::g::Result<()> {
        assert_eq!(cli.recv_seq(), req.idempotent());
        assert_eq!("Hello world".to_string(), std::str::from_utf8(req.data()).unwrap());
        assert_eq!(1, req.id());
        if req.idempotent() == 10000 {
            cli.shutdown();
        }
        Ok(())
    }
}

async fn work() {
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
    let cli = client::Client::<()>::new_arc(shutdown_tx);
    let controller = cli.clone();

    let j = tokio::spawn(async move {
        tg::nw::tcp::client_run::<EchoEvent>("127.0.0.1:6688", cli).await;
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

#[tokio::main]
async fn main() {
    tg::utils::init_log();

    let mut arr = Vec::new();
    for _ in 0..100 {
        let j = tokio::spawn(async move {
            work().await;
        });
        arr.push(j);
    }

    futures_util::future::join_all(arr).await;
}
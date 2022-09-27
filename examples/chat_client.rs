use async_trait::async_trait;
use std::sync::Arc;
use tg::{g, nw::pack, utils};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::broadcast,
};

#[derive(Default, Clone, Copy)]
struct ChatEvent;

#[async_trait]
impl tg::nw::IClientEvent for ChatEvent {
    type U = ();

    async fn on_process(
        &self,
        cli: &tg::nw::Client<()>,
        req: &pack::Package,
    ) -> g::Result<Option<pack::Response>> {
        tracing::debug!(
            "from server[{:?}]: {}",
            cli.local(),
            std::str::from_utf8(req.data()).unwrap()
        );

        Ok(None)
    }
}

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let mut reader = BufReader::new(tokio::io::stdin());
    let mut line = String::new();
    let (tx, _) = broadcast::channel(1);
    let (p, c) = async_channel::bounded(g::DEFAULT_CHAN_SIZE);

    let cli = tg::nw::Client::<()>::new(30, c, tx.subscribe(), None);
    let cli = Arc::new(cli);

    tokio::spawn(async move {
        tg::nw::tcp::client_run::<ChatEvent>("127.0.0.1:6688", cli).await;
    });

    let mut idempotent = 1;
    'stdin_loop: loop {
        let result_read = reader.read_line(&mut line).await;
        if let Err(err) = result_read {
            tracing::error!("{:?}", err);
            break 'stdin_loop;
        }

        let data = line.trim();
        if data.to_ascii_lowercase() == "exit" {
            tracing::debug!("...");
            tx.send(1).unwrap();
            break 'stdin_loop;
        }

        let mut req = pack::REQ_POOL.pull();
        req.set_service_id(1);
        req.set_package_id(1);
        req.set_idempotent(idempotent);
        req.set_data(data.as_bytes());

        let mut wbuf = pack::WBUF_POOL.pull();
        req.to_bytes(&mut wbuf);

        p.send(Arc::new(wbuf)).await.unwrap();

        line.clear();
        idempotent += 1;
    }

    tracing::debug!("[EXIT].");
}

use async_trait::async_trait;
use std::sync::Arc;
use tg::{g, nw::pack, utils};
use tokio::{
    io::{AsyncBufReadExt, BufReader}
};

#[derive(Default, Clone, Copy)]
struct ChatEvent;

#[async_trait]
impl tg::nw::client::IEvent for ChatEvent {
    type U = ();

    async fn on_process(
        &self,
        cli: &tg::nw::client::Client<()>,
        req: &pack::Package,
    ) -> g::Result<Option<pack::PackBuf>> {
        tracing::debug!("from server[{:?}]: {req}", cli.local());
        Ok(None)
    }
}

#[tokio::main]
async fn main() {
    utils::init_log();

    let mut reader = BufReader::new(tokio::io::stdin());
    let mut line = String::new();
    let (p, c) = async_channel::bounded(g::DEFAULT_CHAN_SIZE);
    let (cli, controller) = tg::nw::client::Client::<()>::new_pair(30, c, None);

    tokio::spawn(async move {
        tg::nw::tcp::client_run::<ChatEvent>("127.0.0.1:6688", cli).await;
    });

    let mut idempotent = 1;

    for i in 0..10000 {
        let mut req = pack::REQ_POOL.pull();
        req.set_package_id(i + 1);
        req.set_idempotent(idempotent);
        req.set_data("Hello world".as_bytes());
        req.setup();

        let mut wbuf = pack::WBUF_POOL.pull();
        req.to_bytes(&mut wbuf);

        p.send(Arc::new(wbuf)).await.unwrap();
        idempotent += 1;
    }

    tracing::info!("开始...");

    'stdin_loop: loop {
        let result_read = reader.read_line(&mut line).await;
        if let Err(err) = result_read {
            tracing::error!("{:?}", err);
            break 'stdin_loop;
        }

        let data = line.trim();
        if data.to_ascii_lowercase() == "exit" {
            tracing::debug!("...");
            controller.shutdown();
            break 'stdin_loop;
        }

        let mut req = pack::REQ_POOL.pull();
        req.set_package_id(1);
        req.set_idempotent(idempotent);
        req.set_data(data.as_bytes());
        req.setup();

        let mut wbuf = pack::WBUF_POOL.pull();
        req.to_bytes(&mut wbuf);

        p.send(Arc::new(wbuf)).await.unwrap();

        line.clear();
        idempotent += 1;
    }

    tracing::debug!("[EXIT].");
}

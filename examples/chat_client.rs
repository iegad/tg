use async_trait::async_trait;
use bytes::BytesMut;
use tg::{g, nw::pack, utils};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::TcpStream,
    sync::broadcast,
};

#[derive(Default, Clone, Copy)]
struct ChatEvent;

#[async_trait]
impl tg::nw::IEvent for ChatEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &tg::nw::ConnPtr<()>,
        req: &pack::Package,
    ) -> g::Result<Option<pack::Response>> {
        tracing::debug!(
            "[{:?}] => {}",
            conn.remote(),
            std::str::from_utf8(req.data()).unwrap()
        );

        Ok(None)
    }
}

struct Client {
    host: &'static str,
    wbuf_tx: async_channel::Sender<BytesMut>,
}

async fn cli_run(
    host: &'static str,
    timeout: u64,
    shutdown_rx: broadcast::Receiver<u8>,
    count: usize,
) -> g::Result<()> {
    Ok(())
}

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let mut reader = BufReader::new(tokio::io::stdin());
    let mut line = String::new();
    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let t = tokio::spawn(async move {});

    'stdin_loop: loop {
        let result_read = reader.read_line(&mut line).await;
        if let Err(err) = result_read {
            tracing::error!("{:?}", err);
            break 'stdin_loop;
        }

        let data = line.trim();
        if data.to_ascii_lowercase() == "exit" {
            tracing::debug!("...");
            shutdown_tx.send(1).unwrap();
            t.await.unwrap();
            break 'stdin_loop;
        }

        line.clear();
    }

    tracing::debug!("[EXIT].");
}

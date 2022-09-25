use async_trait::async_trait;
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use pack::{REQ_POOL, WBUF_POOL};
use std::sync::Arc;
use tg::{g, nw::pack, utils};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::TcpStream,
    sync::broadcast,
};

type Client = tg::nw::Conn<()>;
static mut CLIENT: Option<tg::nw::ConnPtr<()>> = None;

lazy_static! {
    static ref CONN_POOL: LinearObjectPool<Client> = Client::pool();
}

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

    async fn on_connected(&self, conn: &tg::nw::ConnPtr<()>) -> g::Result<()> {
        tracing::debug!("[{} - {:?}] has connected", conn.sockfd(), conn.remote());

        unsafe {
            if CLIENT.is_none() {
                CLIENT = Some(conn.clone());
            }
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let mut reader = BufReader::new(tokio::io::stdin());
    let mut line = String::new();

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let conn = CONN_POOL.pull();

    let t = tokio::spawn(async move {
        let stream = match TcpStream::connect("127.0.0.1:6688").await {
            Err(err) => {
                tracing::error!("{:?}", err);
                std::process::exit(1);
            }
            Ok(v) => v,
        };

        tg::nw::tcp::conn_handle(stream, conn, 0, shutdown_rx, ChatEvent::default()).await;
    });

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

        unsafe {
            if let Some(cli) = CLIENT.as_ref() {
                let mut wbuf = WBUF_POOL.pull();
                let mut req = REQ_POOL.pull();
                req.set_service_id(1);
                req.set_package_id(1);
                req.set_router_id(1);
                req.set_idempotent(cli.send_seq() + 1);
                req.set_data(data.as_bytes());
                req.to_bytes(&mut wbuf);

                if let Err(err) = cli.send(Arc::new(wbuf)) {
                    tracing::error!("{:?}", err);
                    break 'stdin_loop;
                }
            }
        }

        line.clear();
    }

    tracing::debug!("[EXIT].");
}

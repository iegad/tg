use axum::{routing, Router};
use lazy_static::lazy_static;
use tg::{utils, g};
use async_trait::async_trait;
use lockfree_object_pool::LinearObjectPool;

type Conn = tg::nw::Conn<()>;

#[derive(Clone, Default)]
struct ChatEvent;

#[async_trait]
impl tg::nw::IEvent for ChatEvent {
    type U = ();

    async fn on_process(&self, conn: &tg::nw::Conn<()>, req: &tg::nw::pack::Package) -> g::Result<Option<tg::nw::Response>> {
        tracing::debug!("[{} | {:?}] => {:?}", conn.sockfd(), conn.remote(), req.data());
        Ok(None)
    }
}

impl tg::nw::IServerEvent for ChatEvent{}

lazy_static! {
    static ref SERVER: tg::nw::ServerPtr<ChatEvent> = tg::nw::Server::new_ptr("0.0.0.0:6688", g::DEFAULT_MAX_CONNECTIONS, g::DEFAULT_READ_TIMEOUT);
    static ref CONN_POOL: LinearObjectPool<Conn> = Conn::pool();
}

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let app = Router::new()
        .route("/start", routing::get(start))
        .route("/stop", routing::get(stop));
    axum::Server::bind(&("0.0.0.0:80".parse().unwrap()))
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn start() -> &'static str {
    if SERVER.running() {
        return "server is already running."
    }

    if let Err(err) = tg::nw::tcp::server_run(SERVER.clone(), &CONN_POOL).await {
        tracing::error!("{:?}", err);
        return "server internal error.";
    }

    "OK"
}

async fn stop() -> &'static str {
    if !SERVER.running() {
        return "server is not running.";
    }

    SERVER.shutdown();
    SERVER.wait().await;

    "OK"
}

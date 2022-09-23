use async_trait::async_trait;
use axum::{http::StatusCode, response::IntoResponse, routing, Json, Router};
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tg::{g, nw::pack::WBUF_POOL, utils};

type ConnPtr = tg::nw::ConnPtr<()>;

lazy_static! {
    static ref SERVER: tg::nw::ServerPtr<ChatEvent> = tg::nw::Server::new_ptr(
        "0.0.0.0:6688",
        g::DEFAULT_MAX_CONNECTIONS,
        g::DEFAULT_READ_TIMEOUT
    );
    static ref CONN_POOL: LinearObjectPool<tg::nw::Conn<()>> = tg::nw::Conn::pool();
    static ref SESSIONS: Arc<Mutex<HashMap<i32, ConnPtr>>> = Arc::new(Mutex::new(HashMap::new()));
}

#[derive(Clone, Default)]
struct ChatEvent;

#[async_trait]
impl tg::nw::IEvent for ChatEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &ConnPtr,
        req: &tg::nw::pack::Package,
    ) -> g::Result<Option<tg::nw::Response>> {
        assert_eq!(req.idempotent(), conn.recv_seq());

        let mut wbuf = WBUF_POOL.pull();
        req.to_bytes(&mut wbuf);
        Ok(Some(Arc::new(wbuf)))
    }

    async fn on_connected(&self, conn: &ConnPtr) -> g::Result<()> {
        SESSIONS.lock().unwrap().insert(conn.sockfd(), conn.clone());
        tracing::debug!("[{}] has connected", conn.clone().sockfd());
        Ok(())
    }

    async fn on_disconnected(&self, conn: &ConnPtr) {
        SESSIONS.lock().unwrap().remove(&conn.sockfd());
        tracing::debug!("[{}] has disconnected", conn.sockfd());
    }
}

impl tg::nw::IServerEvent for ChatEvent {}

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let app = Router::new()
        .route("/start", routing::get(start))
        .route("/stop", routing::get(stop))
        .route("/kick", routing::post(kick));
    axum::Server::bind(&("0.0.0.0:8088".parse().unwrap()))
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn start() -> &'static str {
    if SERVER.running() {
        return "server is already running.";
    }

    tokio::spawn(async move {
        if let Err(err) = tg::nw::tcp::server_run(SERVER.clone(), &CONN_POOL).await {
            tracing::error!("{:?}", err);
        }
    });

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

async fn kick(Json(req): Json<KickReq>) -> impl IntoResponse {
    if let Some(conn) = SESSIONS.lock().unwrap().get(&req.id) {
        conn.shutdown();
    }

    let rsp = KickRsp {
        code: 0,
        error: "".to_string(),
    };

    (StatusCode::CREATED, Json(rsp))
}

#[derive(Deserialize, Serialize, Debug)]
pub struct KickReq {
    id: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KickRsp {
    code: i32,
    error: String,
}

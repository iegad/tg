use std::net::SocketAddr;

use crate::server::{CONN_POOL, SERVER};

use super::CommonRsp;
use axum::{extract::ConnectInfo, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct RunReq {
    run: bool,
}

pub async fn handle(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<RunReq>,
) -> impl IntoResponse {
    tracing::debug!("[{addr}] POST /run");

    let mut rsp = CommonRsp::<()> {
        code: 0,
        error: None,
        data: None,
    };

    if req.run {
        if SERVER.running() {
            rsp.code = -1;
            rsp.error = Some("server is already running.".to_string());
        } else {
            let server = SERVER.clone();
            tokio::spawn(async move {
                if let Err(err) = tg::nw::tcp::server_run(server, &CONN_POOL).await {
                    tracing::error!("server run: {:?}", err);
                }
            });
        }
    } else {
        if !SERVER.running() {
            rsp.code = -1;
            rsp.error = Some("server is not running.".to_string());
        } else {
            SERVER.shutdown();
            SERVER.wait().await;
        }
    }

    (StatusCode::OK, Json(rsp))
}

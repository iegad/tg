use std::net::SocketAddr;

use axum::{extract::ConnectInfo, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::{manager::CommonRsp, server::SESSIONS};

#[derive(Debug, Deserialize, Serialize)]
pub struct KickReq {
    #[cfg(unix)]
    sockfd: i32,
    #[cfg(windows)]
    sockfd: u64,
}

pub async fn handle(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<KickReq>,
) -> impl IntoResponse {
    tracing::debug!("[{addr}] POST /kick");

    let mut rsp = CommonRsp::<()>::new();

    match SESSIONS.lock().unwrap().get(&req.sockfd) {
        Some(v) => v.shutdown(),
        None => {
            rsp.code = -1;
            rsp.error = Some("connections is not exists".to_string());
        }
    }

    (StatusCode::OK, Json(rsp))
}

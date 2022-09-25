use super::CommonRsp;
use crate::server::{SERVER, SESSIONS};
use axum::{extract::ConnectInfo, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ServerInfo {
    host: &'static str,
    max_connections: usize,
    timeout: u64,
    running: bool,
    sessions: Vec<ConnInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ConnInfo {
    #[cfg(unix)]
    sockfd: i32,
    #[cfg(windows)]
    sockfd: u64,
    remote: SocketAddr,
}

static mut SERVER_INFO: Option<ServerInfo> = None;

pub async fn handle(ConnectInfo(addr): ConnectInfo<SocketAddr>) -> impl IntoResponse {
    tracing::debug!("[{addr}] POST /info");

    unsafe {
        if let None = SERVER_INFO {
            SERVER_INFO = Some(ServerInfo {
                host: SERVER.host(),
                max_connections: SERVER.max_connections(),
                running: false,
                timeout: SERVER.timeout(),
                sessions: Vec::new(),
            });
        }
    }

    let mut serv_info = unsafe { SERVER_INFO.as_mut().unwrap().clone() };
    serv_info.running = SERVER.running();
    {
        let guard = SESSIONS.lock().unwrap();
        for i in guard.iter() {
            serv_info.sessions.push(ConnInfo {
                sockfd: i.1.sockfd(),
                remote: i.1.remote().clone(),
            })
        }
    }

    let rsp = CommonRsp::<ServerInfo> {
        code: 0,
        error: None,
        data: Some(serv_info),
    };

    (StatusCode::OK, Json(rsp))
}

mod manager;
mod server;

use axum::{routing, Router};
use std::net::SocketAddr;
use tg::utils;

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    tracing::debug!("{}", utils::get_pwd().unwrap());

    let app = Router::new()
        .route("/run", routing::post(manager::run::handle))
        .route("/info", routing::post(manager::info::handle))
        .route("/kick", routing::post(manager::kick::handle));

    axum::Server::bind(&("0.0.0.0:8088".parse().unwrap()))
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .unwrap();
}

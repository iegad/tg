use axum::{routing, Router};
use tg::utils;
use user::server::UserServer;
mod user;

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let app = Router::new()
        .route("/", routing::get(home))
        .route("/start", routing::get(start))
        .route("/stop", routing::get(stop));
    axum::Server::bind(&("0.0.0.0:80".parse().unwrap()))
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn home() -> &'static str {
    "Hello world"
}

async fn start() -> &'static str {
    let server = UserServer::instance().server();

    if server.running() {
        return "UserServer is already running";
    }

    tokio::spawn(async move {
        UserServer::run(server);
    });
    "OK"
}

async fn stop() -> &'static str {
    let server = UserServer::instance().server();

    if !server.running() {
        return "UserServer is not running yet";
    }

    UserServer::stop(server);
    "OK"
}

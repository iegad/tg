use async_trait::async_trait;
use bytes::Bytes;
use tg::{nw::pack, utils};

#[derive(Clone, Copy, Default)]
struct EchoEvent;

#[async_trait]
impl tg::nw::IEvent for EchoEvent {
    async fn on_process(
        &self,
        conn: &tg::nw::Conn,
        req: &pack::Package,
    ) -> tg::g::Result<Option<Bytes>> {
        assert_eq!(req.idempotent(), conn.recv_seq());
        Ok(Some(req.to_bytes().freeze()))
    }

    async fn on_disconnected(&self, conn: &tg::nw::Conn) {
        tracing::debug!(
            "[{}|{:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
    }
}

#[async_trait]
impl tg::nw::IServerEvent for EchoEvent {}

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let server = tg::nw::Server::<EchoEvent>::new("0.0.0.0:6688", 100, tg::g::DEFAULT_READ_TIMEOUT);
    if let Err(err) = tg::nw::tcp::server_run(server.clone()).await {
        println!("{:?}", err);
    }
}

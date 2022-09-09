use async_trait::async_trait;
use bytes::Bytes;
use tg::nw::pack;

#[derive(Clone, Copy)]
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
        println!(
            "[{}|{:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
        // assert_eq!(1_000_000, conn.recv_seq());
    }
}

#[async_trait]
impl tg::nw::IServerEvent for EchoEvent {}

#[tokio::main]
async fn main() {
    let server = tg::nw::Server::new(
        "0.0.0.0:6688",
        100,
        tg::g::DEFAULT_READ_TIMEOUT,
        EchoEvent {},
    );

    if let Err(err) = tg::nw::tcp::server_run(&server).await {
        println!("{:?}", err);
    }
}

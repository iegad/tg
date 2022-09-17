use async_trait::async_trait;
use bytes::Bytes;
use tg::nw::pack;

#[derive(Clone, Copy, Default)]
struct SimpleEvent;

#[async_trait]
impl tg::nw::IEvent for SimpleEvent {
    async fn on_process(
        &self,
        conn: &tg::nw::Conn,
        req: &pack::Package,
    ) -> tg::g::Result<Option<Bytes>> {
        assert_eq!(req.idempotent(), conn.recv_seq());
        Ok(None)
    }

    async fn on_disconnected(&self, conn: &tg::nw::Conn) {
        println!(
            "[{}|{:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
        assert_eq!(10000, conn.recv_seq());
    }
}

#[async_trait]
impl tg::nw::IServerEvent for SimpleEvent {}

#[tokio::main]
async fn main() {
    let server =
        tg::nw::Server::<SimpleEvent>::new("0.0.0.0:6688", 100, tg::g::DEFAULT_READ_TIMEOUT);

    let controller = server.clone();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        assert!(controller.running());
        controller.shutdown();
    });

    if let Err(err) = tg::nw::tcp::server_run(server).await {
        println!("{:?}", err);
    }
}

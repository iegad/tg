use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};

#[derive(Clone, Copy)]
struct EchoEvent;

#[async_trait]
impl tg::nw::IEvent for EchoEvent {
    async fn on_process(
        &self,
        conn: &tg::nw::Conn,
        req: &BytesMut,
    ) -> tg::g::Result<Option<Bytes>> {
        println!(
            "[{}|{:?}] => {}",
            conn.sockfd(),
            conn.remote(),
            std::str::from_utf8(&req[..req.len()]).unwrap()
        );

        let mut b = BytesMut::new();
        b.put(&req[..req.len()]);
        Ok(Some(b.freeze()))
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

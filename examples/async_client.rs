use std::time::Duration;

use async_trait::async_trait;
use bytes::BytesMut;
use tg::{
    g,
    nw::{self, pack::RSP_POOL},
};
use tokio::{sync::broadcast, time};

#[derive(Clone, Copy)]
struct EchoProc;

#[async_trait]
impl nw::ICliProc for EchoProc {
    async fn on_process(&self, req: &nw::pack::Package) -> g::Result<Option<BytesMut>> {
        println!(">> => idempotent: {}, {}", req.idempotent(), unsafe {
            std::str::from_utf8_unchecked(req.data())
        });

        let mut rsp = RSP_POOL.pull();
        rsp.set_service_id(req.service_id());
        rsp.set_idempotent(req.idempotent() + 1);
        rsp.set_package_id(req.package_id());
        rsp.set_token(req.token());
        rsp.set_data(req.data());
        Ok(Some(rsp.to_bytes()))
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let cli = tg::nw::tcp::Client::new("127.0.0.1:6688").await.unwrap();
    let tx = cli.sender();
    let (notify_shutdown, shutdown) = broadcast::channel(1);

    tokio::spawn(async move {
        tg::nw::tcp::client_run(cli, shutdown, EchoProc {}).await;
    });

    time::sleep(Duration::from_secs(5)).await;

    let req = nw::pack::Package::with_params(1, 2, 3, 1, 4, b"Hello world");
    if let Err(err) = tx.send(req.to_bytes()) {
        panic!("{:?}", err);
    }

    time::sleep(Duration::from_secs(2)).await;
    notify_shutdown.send(1).unwrap();

    Ok(())
}

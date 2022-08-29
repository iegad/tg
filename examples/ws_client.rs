use futures_util::{future, SinkExt, StreamExt};
use tg::{g, nw};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use url::Url;

static STR: &str = "Hello world";

async fn work() {
    let data = STR.as_bytes();

    let url = Url::parse("ws://127.0.0.1:6688/").unwrap();
    let (mut sock, _) = connect_async(url).await.unwrap();

    let mut rsp = nw::pack::PACK_POOL.pull();

    for i in 0..10000 {
        let mut pack = nw::pack::RSP_POOL.pull();
        pack.set_service_id(1);
        pack.set_router_id(2);
        pack.set_package_id(3);
        pack.set_token(4);
        pack.set_idempotent(i + 1);
        pack.set_data(data);

        let wbuf = pack.to_bytes();
        if let Err(err) = sock.send(Message::binary(wbuf.to_vec())).await {
            println!("Err => {:?}", err);
            break;
        };

        let msg = match sock.next().await {
            Some(v) => match v {
                Ok(v) => v,
                Err(err) => panic!("{:?}", err),
            },
            None => panic!("read none"),
        };

        match msg {
            Message::Binary(v) => {
                assert!(rsp.parse(&v).unwrap());
                assert_eq!(rsp.service_id(), 1);
                assert_eq!(rsp.router_id(), 2);
                assert_eq!(rsp.package_id(), 3);
                assert_eq!(rsp.idempotent(), 1 + i);
                assert_eq!(rsp.data().len(), pack.data().len());
                assert_eq!(rsp.token(), 4);
                assert_eq!(STR, core::str::from_utf8(rsp.data()).unwrap());
            }

            _ => panic!("failed"),
        }
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let beg = tg::utils::now_unix_micros();
    let mut arr = vec![];

    for _ in 0..100 {
        arr.push(tokio::spawn(async move {
            work().await;
        }));
    }

    future::join_all(arr).await;
    println!("done.... 耗时 {} ms", tg::utils::now_unix_micros() - beg);
    Ok(())
}

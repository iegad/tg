use bytes::BytesMut;
use std::sync::Arc;
use tg::{g, nw};
use tokio::{io::AsyncWriteExt, sync::Semaphore};

static STR: &str = "Hello world";

async fn work() {
    let data = STR.as_bytes();

    let mut sock = match tokio::net::TcpStream::connect("127.0.0.1:6688").await {
        Err(err) => panic!("connect failed: {:?}", err),
        Ok(v) => v,
    };

    let mut rbuf = BytesMut::with_capacity(4096);
    let mut rsp = nw::pack::PACK_POOL.pull();

    for i in 0..10000 {
        let mut pack = nw::pack::PACK_POOL.pull();
        pack.set_service_id(1);
        pack.set_router_id(2);
        pack.set_package_id(3);
        pack.set_token(4);
        pack.set_idempotent(i + 1);
        pack.set_data(data);

        let wbuf = pack.to_bytes();
        if let Err(err) = sock.write_all(&wbuf[..wbuf.len()]).await {
            println!("Err => {:?}", err);
            break;
        };

        match nw::tcp::read(&mut sock, &mut rbuf, &mut rsp).await {
            Ok(0) => panic!("-- EOF --"),
            Ok(_) => {
                assert_eq!(rsp.service_id(), 1);
                assert_eq!(rsp.router_id(), 2);
                assert_eq!(rsp.package_id(), 3);
                assert_eq!(rsp.idempotent(), 1 + i);
                assert_eq!(rsp.data().len(), pack.data().len());
                assert_eq!(rsp.token(), 4);
                assert_eq!(STR, core::str::from_utf8(rsp.data()).unwrap());
            }

            Err(err) => panic!("{:?}", err),
        }

        rbuf.clear();
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let beg = tg::utils::now_unix_micros();
    let count = Arc::new(Semaphore::new(10));

    for _ in 0..100 {
        let permit = count.clone().acquire_owned().await.unwrap();
        tokio::spawn(async move {
            work().await;
            drop(permit);
        });
    }

    while count.available_permits() < 10 {}
    println!("done.... 耗时 {} ms", tg::utils::now_unix_micros() - beg);
    Ok(())
}

use std::sync::Arc;
use tg::{g, nw};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Semaphore,
};

async fn work() {
    let s = "Hello world";

    let data = s.as_bytes();

    let mut sock = match tokio::net::TcpStream::connect("127.0.0.1:6688").await {
        Err(err) => panic!("connect failed: {:?}", err),
        Ok(v) => v,
    };

    let mut rsp = nw::pack::Package::new();

    'ntime: for i in 0..10000 {
        let pack = nw::pack::Package::with_params(1, i + 1, data);

        if let Err(err) = sock.write_all(pack.to_bytes()).await {
            println!("Err => {:?}", err);
            break;
        };

        loop {
            let n = match sock.read(rsp.as_bytes()).await {
                Ok(0) => {
                    println!("EOF");
                    break 'ntime;
                }
                Ok(n) => n,
                Err(err) => {
                    panic!("Err => {:?}", err);
                }
            };

            let ok = rsp.parse(n).unwrap();

            if ok {
                assert_eq!(s, core::str::from_utf8(rsp.data()).unwrap());
                break;
            }
        }
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let beg = tg::utils::now_unix();
    let count = Arc::new(Semaphore::new(10));

    for _ in 0..100 {
        let permit = count.clone().acquire_owned().await.unwrap();
        tokio::spawn(async move {
            work().await;
            drop(permit);
        });
    }

    while count.available_permits() < 10 {}
    println!("done.... 耗时 {}s", tg::utils::now_unix() - beg);
    Ok(())
}

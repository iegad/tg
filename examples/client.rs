use std::sync::Arc;
use tg::{g, nw};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Semaphore,
};

static STR: &str = "Hello world";

async fn work() {
    let data = STR.as_bytes();

    let mut sock = match tokio::net::TcpStream::connect("127.0.0.1:6688").await {
        Err(err) => panic!("connect failed: {:?}", err),
        Ok(v) => v,
    };

    let mut rsp = nw::pack::PACK_POOL.pull();

    'ntime: for i in 0..10000 {
        let mut pack = nw::pack::PACK_POOL.pull();
        pack.set_pid(1);
        pack.set_idempotent(i + 1);
        pack.set_data(data);

        if let Err(err) = sock.write_all(pack.to_bytes()).await {
            println!("Err => {:?}", err);
            break;
        };

        loop {
            let n = match sock.read(rsp.as_mut_bytes()).await {
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
                assert_eq!(STR, core::str::from_utf8(rsp.data()).unwrap());
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

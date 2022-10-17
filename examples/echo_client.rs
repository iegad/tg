use std::sync::Arc;

use futures_util::future;
use tg::{g, nw::pack::{WBUF_POOL, REQ_POOL, RSP_POOL}, utils};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

async fn work(host: Arc<String>) {
    let mut cli = TcpStream::connect(&*host).await.unwrap();
    let data = b"1234567890";

    let (mut reader, mut writer) = cli.split();
    for i in 0..10000 {
        let mut req = REQ_POOL.pull();
        req.set(1, i + 1, data);
        
        let mut wbuf = WBUF_POOL.pull();
        req.to_bytes(&mut wbuf);
        if let Err(err) = writer.write_all(&wbuf).await {
            println!("write failed: {:?}", err);
            break;
        }

        let mut rbuf = [0u8; g::DEFAULT_BUF_SIZE];
        let mut rsp = RSP_POOL.pull();

        'reader_loop: loop {
            let nread = match reader.read(&mut rbuf).await {
                Ok(v) => v,
                Err(err) => panic!("{:?}", err),
            };

            let mut consume = 0;
            'pack_loop: loop {
                if rsp.valid() {
                    rsp.reset();
                    break 'reader_loop;
                } else {
                    consume += match rsp.from_bytes(&rbuf[consume..nread]) {
                        Err(err) => panic!("{err}"),
                        Ok(v) => v,
                    };
                }

                if consume == nread && !rsp.valid() {
                    break 'pack_loop;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    utils::init_log();

    let mut arr = Vec::new();
    let beg = utils::now_unix_micros();
    let host = Arc::new(std::env::args().nth(1).unwrap());
    for _ in 0..100 {
        let v = host.clone();
        arr.push(tokio::spawn(async move {
            work(v).await;  
        }));
    }

    future::join_all(arr).await;
    println!("done....!!!\n 总耗时: {} micro seconds", utils::now_unix_micros() - beg);
}

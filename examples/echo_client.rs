use std::sync::Arc;
use futures_util::future;
use tg::{g, nw::pack::{REQ_POOL, RSP_POOL, WBUF_POOL}, utils};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

static DATA: &[u8] = b"1234567890";

async fn work(host: Arc<String>) {
    let stream = TcpStream::connect(&*host).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let j = tokio::spawn(async move {
        let mut buf =vec![0u8; g::DEFAULT_BUF_SIZE];
        let mut pck = RSP_POOL.pull();
        let mut rc = 0;

        'read_loop: loop {
            let nread = match reader.read(&mut buf).await {
                Ok(0) => {
                    tracing::info!("EOF.");
                    break 'read_loop
                }
                Ok(n) => n,
                Err(err) => {
                    tracing::error!("read failed: {err}");
                    break 'read_loop;
                }
            };

            let mut consume = 0;
            'pack_loop: loop {
                if pck.valid() {
                    rc += 1;

                    assert_eq!(std::str::from_utf8(pck.data()).unwrap(), std::str::from_utf8(DATA).unwrap());

                    if pck.idempotent() == 10000 {
                        break 'read_loop;
                    }
                    
                    pck.reset();
                } else {
                    consume += match pck.from_bytes(&buf[consume..nread]) {
                        Ok(n) => n,
                        Err(err) => {
                            tracing::error!("{} => {err}", line!());
                            break 'read_loop;
                        }
                    };
                }

                if consume == nread && !pck.valid() {
                    break 'pack_loop;
                }
            }
        }

        assert_eq!(rc, 10000);
    });

    for i in 0..10000 {
        let mut req = REQ_POOL.pull();
        req.set(1, i + 1, DATA);
        
        let mut wbuf = WBUF_POOL.pull();
        req.to_bytes(&mut wbuf);
        if let Err(err) = writer.write_all(&wbuf).await {
            println!("write failed: {:?}", err);
            break;
        }
    }

    j.await.unwrap();
    
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

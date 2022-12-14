use std::sync::Arc;
use futures_util::future;
use tg::{g, utils, nw::packet::REQ_POOL};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

static NTIME: usize = 10000;
static DATA: &[u8] = b"1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567";

async fn work(host: Arc<String>) {
    let stream = TcpStream::connect(&*host).await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    // tracing::info!("????????????");

    let hstr = hex::encode(DATA);

    let j = tokio::spawn(async move {
        let mut buf =vec![0u8; g::DEFAULT_BUF_SIZE];
        let mut rc = 0;
        let mut builer = tg::nw::packet::Builder::new();
        
        'read_loop: loop {
            let n = match reader.read(&mut buf).await {
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

            'pack_loop: loop {
                let option_pkt = match builer.parse(&buf[..n]) {
                    Err(err) => {
                        tracing::error!("{err}");
                        break 'read_loop;
                    }
                    Ok(v) => v,
                };

                let (p, next) = match option_pkt {
                    Some(v) => v,
                    None => break 'pack_loop,
                };

                assert_eq!(p.id(), 1);
                assert_eq!(p.idempotent(), rc + 1);
                assert_eq!(hstr, hex::encode(p.data()));
                rc = p.idempotent();

                if !next {
                    break 'pack_loop;
                }
            }

            if rc as usize == NTIME {
                break 'read_loop;
            }
        }

        assert_eq!(rc as usize, NTIME);
    });

    for i in 0..NTIME {
        let mut req = REQ_POOL.pull();
        req.set(1, i as u32 + 1, DATA);

        if let Err(err) = writer.write_all(req.raw()).await {
            println!("write failed: {:?}", err);
            break;
        }
    }

    // tracing::info!("????????????");
    j.await.unwrap();
}

#[tokio::main]
async fn main() {
    utils::init_log();

    let mut arr = Vec::new();
    let host = Arc::new(std::env::args().nth(1).unwrap());

    let beg = utils::now_unix_micros();
    for _ in 0..100 {
        let v = host.clone();
        arr.push(tokio::spawn(async move {
            work(v).await;  
        }));
    }

    future::join_all(arr).await;
    println!("done....!!!\n ?????????: {} micro seconds", utils::now_unix_micros() - beg);
}

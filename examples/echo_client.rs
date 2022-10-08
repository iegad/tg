use futures_util::future;
use tg::{g, nw::pack::WBUF_POOL, utils};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

async fn work() {
    let mut cli = TcpStream::connect("127.0.0.1:6688").await.unwrap();
    let data = b"Hello world";

    let (mut reader, mut writer) = cli.split();
    for i in 0..10000 {
        let req = tg::nw::pack::Package::with_params( 1, i + 1, data);
        let mut wbuf = WBUF_POOL.pull();
        req.to_bytes(&mut wbuf).unwrap();
        if let Err(err) = writer.write_all_buf(&mut *wbuf).await {
            println!("write failed: {:?}", err);
            break;
        }

        let mut rbuf = [0u8; g::DEFAULT_BUF_SIZE];
        let mut rsp = tg::nw::pack::Package::new();

        'reader_loop: loop {
            let nread = match reader.read(&mut rbuf).await {
                Ok(v) => v,
                Err(err) => panic!("{:?}", err),
            };

            // tracing::debug!("读取: {} 节字", nread);
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
    for _ in 0..100 {
        arr.push(tokio::spawn(async move {
            work().await;
        }));
    }

    future::join_all(arr).await;
    println!(
        "done....!!!\n 总耗时: {} micro seconds",
        utils::now_unix_micros() - beg
    );
}

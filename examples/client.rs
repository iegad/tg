use futures_util::future;
use tg::utils;
use tokio::{io::AsyncWriteExt, net::TcpStream};

async fn work() {
    let mut cli = TcpStream::connect("127.0.0.1:6688").await.unwrap();

    let (_, mut writer) = cli.split();
    for i in 0..1_000_000 {
        let data = format!("Hello world: {}", i);
        let req = tg::nw::pack::Package::with_params(1, 1, 1, i + 1, data.as_bytes());
        let mut wbuf = req.to_bytes();
        if let Err(err) = writer.write_all_buf(&mut wbuf).await {
            println!("write failed: {:?}", err);
            break;
        }
    }
}

#[tokio::main]
async fn main() {
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

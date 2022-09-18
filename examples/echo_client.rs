use bytes::BytesMut;
use futures_util::future;
use tg::{g, utils};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

async fn work() {
    let mut cli = TcpStream::connect("127.0.0.1:6688").await.unwrap();
    let data = b"Hello world";

    let (mut reader, mut writer) = cli.split();
    for i in 0..10000 {
        let req = tg::nw::pack::Package::with_params(1, 1, 1, i + 1, data);
        let mut wbuf = req.to_bytes();
        if let Err(err) = writer.write_all_buf(&mut wbuf).await {
            println!("write failed: {:?}", err);
            break;
        }

        let mut rbuf = BytesMut::with_capacity(g::DEFAULT_BUF_SIZE);
        let mut rsp = tg::nw::pack::Package::new();
        'reader_loop: loop {
            match reader.read_buf(&mut rbuf).await {
                Ok(_) => (),
                Err(err) => panic!("{:?}", err),
            };

            'rsp_loop: loop {
                if let Err(err) = tg::nw::pack::Package::parse(&mut rbuf, &mut rsp) {
                    println!("{:?}", err);
                    break 'reader_loop;
                }

                if rsp.valid() {
                    assert_eq!(rsp.service_id(), req.service_id());
                    assert_eq!(rsp.package_id(), req.package_id());
                    assert_eq!(rsp.idempotent(), req.idempotent());
                    assert_eq!(rsp.router_id(), req.router_id());
                    assert_eq!(data.len() + tg::nw::pack::Package::HEAD_SIZE, rsp.raw_len());
                    assert_eq!(
                        std::str::from_utf8(data).unwrap(),
                        std::str::from_utf8(req.data()).unwrap()
                    );
                    break 'reader_loop;
                } else {
                    if rbuf.len() < tg::nw::pack::Package::HEAD_SIZE {
                        break 'rsp_loop;
                    }
                }
            }
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

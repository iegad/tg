use bytes::BytesMut;
use tg::{g, nw::pack::PACK_POOL};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[tokio::main]
async fn main() -> g::Result<()> {
    let beg = tg::utils::now_unix_micros();

    let mut conn = TcpStream::connect("127.0.0.1:6688").await.unwrap();

    let mut req = PACK_POOL.pull();
    req.set_service_id(1);
    req.set_package_id(1);
    req.set_data(b"Hello world");

    conn.write_all(&req.to_bytes()).await.unwrap();

    let mut rbuf = BytesMut::with_capacity(g::DEFAULT_BUF_SIZE);
    let _n = conn.read_buf(&mut rbuf).await.unwrap();
    let mut rsp = PACK_POOL.pull();
    let _res = rsp.parse(&rbuf).unwrap();

    println!("{}", core::str::from_utf8(rsp.data()).unwrap());

    println!("done.... 耗时 {} ms", tg::utils::now_unix_micros() - beg);
    Ok(())
}

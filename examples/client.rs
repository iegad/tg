use tg::{g, nw};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> g::Result<()> {
    let s = "1234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890";

    let data = s.as_bytes();

    let mut sock = match tokio::net::TcpStream::connect("127.0.0.1:6688").await {
        Err(err) => panic!("connect failed: {:?}", err),
        Ok(v) => v,
    };

    let mut rbuf = vec![0u8; 1500];
    let mut in_pack = nw::pack::Package::new();

    let beg = tg::utils::now_unix_nanos();
    for i in 0..100 {
        let wbuf = nw::pack::Package::with_params(1, i, 3, data).to_bytes();

        if let Err(err) = sock.write_all(&wbuf).await {
            println!("Err => {:?}", err);
            break;
        };

        loop {
            let n = match sock.read(&mut rbuf).await {
                Ok(0) => {
                    println!("EOF");
                    break;
                }
                Ok(n) => n,
                Err(err) => {
                    println!("Err => {:?}", err);
                    break;
                }
            };

            let ok = in_pack.parse(&rbuf[..n]).unwrap();

            if ok {
                assert_eq!(s, core::str::from_utf8(in_pack.data()).unwrap());
                in_pack.clear();
                break;
            }
        }
    }

    sock.shutdown().await.unwrap();
    println!("done.... 耗时{} us", tg::utils::now_unix_nanos() - beg);
    Ok(())
}

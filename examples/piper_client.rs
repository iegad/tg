use tg::{g, nw::pack::RSP_POOL};
use tokio::net::TcpStream;

static DATA: &[u8; 11] = b"Hello world";

#[tokio::main]
async fn main() -> g::Result<()> {
    let beg = tg::utils::now_unix_micros();

    let mut conn = TcpStream::connect("127.0.0.1:6688").await.unwrap();

    for i in 0..1000 {
        match tg::piper::call(&mut conn, 1, 1, 0, i + 1, 1, DATA).await {
            Err(err) => {
                println!("{:?}", err);
                break;
            }

            Ok(None) => {
                println!("-- EOF --");
                break;
            }

            Ok(v) => {
                let v = v.unwrap();
                let mut rsp = RSP_POOL.pull();
                let res = match rsp.parse(&v) {
                    Ok(res) => res,
                    Err(err) => {
                        println!("{:?}", err);
                        break;
                    }
                };

                if !res {
                    panic!("oh shit....");
                }

                println!("{}", core::str::from_utf8(rsp.data()).unwrap());
            }
        }
    }

    println!("done.... 耗时 {} ms", tg::utils::now_unix_micros() - beg);
    Ok(())
}

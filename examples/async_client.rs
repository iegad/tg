use tokio::{net::TcpStream, io::AsyncReadExt};

lazy_static::lazy_static! {
    static ref STREAM: TcpStream = {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let cli = match TcpStream::connect("127.0.0.1:6688").await {
                Ok(v) => v,
                Err(err) => panic!("{err}"),
            };

            cli
        })
    };
}

#[tokio::main]
async fn main() {
    tg::utils::init_log();
    let (mut reader, _writer) = unsafe {
        let p = &mut *(&*STREAM as *const TcpStream as *mut TcpStream);
        p.split()
    };
    
    let rtsk = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];

        loop {
            let _n = match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) => {
                    tracing::error!("{err}");
                    break;
                }
            };
        }
    });

    rtsk.await.unwrap();
}
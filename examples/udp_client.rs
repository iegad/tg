use std::net::SocketAddr;

use tokio::net::UdpSocket;

#[tokio::main]
async fn main() {
    tg::utils::init_log();

    let host = match std::env::args().nth(1) {
        Some(v) => v,
        None => {
            tracing::error!("not main args");
            return;
        }
    };

    let sock = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    tracing::debug!("bind ok: {:?}", sock.local_addr().unwrap());

    let target: SocketAddr = host.parse().unwrap();
    
    let mut buf = vec![0u8; 2046];

    let mut total = 0;

    for i in 0..100000 {
        let data = format!("Hello world: {i}");

        match sock.send_to(data.as_bytes(), target).await {
            Ok(n) => {
                if n != data.len() {
                    tracing::error!("send data bytes {n} not {}", data.len());
                    break;
                }

                match sock.recv_from(&mut buf).await {
                    Ok((n, from)) => {
                        let tmp = std::str::from_utf8(&buf[..n]).unwrap();
                        if tmp != data {
                            tracing::error!("recv {tmp} <> send {data}");
                            break;
                        }
                        tracing::debug!("[{from}] => {tmp}");
                        total += 1;
                    }

                    Err(err) => {
                        tracing::error!("recv from failed: {err}");
                    }
                }
            }
            Err(err) => {
                tracing::error!("send to failed: {err}");
                break;
            }
        }
    }

    tracing::debug!("total: {total}");
}
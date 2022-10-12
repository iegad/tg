use tokio::net::UdpSocket;

#[tokio::main]
async fn main() {
    tg::utils::init_log();

    let sock = UdpSocket::bind("0.0.0.0:3333").await.unwrap();
    tracing::info!("bind ok: {:?}", sock.local_addr().unwrap());

    let mut buf = vec![0u8; 2048];
    loop {
        match sock.recv_from(&mut buf).await {
            Ok((n, from)) => {
                tracing::info!("[{from}]: {}", std::str::from_utf8(&buf[..n]).unwrap());

                match sock.send_to(&buf[..n], &from).await {
                    Ok(v) => {
                        if v != n {
                            tracing::error!("send data bytes: {v} not {n}");
                        }
                    }
                    Err(err) => {
                        tracing::error!("recv from failed: {err}");
                    }
                };
            }
            Err(err) => {
                tracing::error!("recv from failed: {err}");
            }
        };
    }
}
use async_trait::async_trait;
use pack::WBUF_POOL;
use tg::{nw::pack::{self, RspBuf}, utils, make_wbuf};
use tokio::{net::TcpSocket, io::{AsyncReadExt, AsyncWriteExt}};

#[derive(Clone, Default)]
struct EchoEvent;

#[async_trait]
impl tg::nw::server::IEvent for EchoEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &tg::nw::conn::ConnPtr<()>,
        req: &pack::Package,
    ) -> tg::g::Result<RspBuf> {
        assert_eq!(req.idempotent(), conn.recv_seq());
        let mut wbuf = WBUF_POOL.pull();
        tracing::debug!("{}", req);
        req.to_bytes(&mut wbuf);
        Ok(make_wbuf!(wbuf))
    }

    async fn on_disconnected(&self, conn: &tg::nw::conn::ConnPtr<()>) {
        tracing::info!(
            "[{} - {:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
    }
}

#[tokio::main]
async fn main() {
    utils::init_log();
    // tg::tcp_server_run!("0.0.0.0:6688", 100, tg::g::DEFAULT_READ_TIMEOUT, EchoEvent);

    let listener = TcpSocket::new_v4().unwrap();
    listener.set_reuseaddr(true).unwrap();

    listener.bind("0.0.0.0:6688".parse().unwrap()).unwrap();
    let listener = listener.listen(1024).unwrap();
    tracing::debug!("开启监听...");

    loop {
        let (stream, _) = listener.accept().await.unwrap();

        tokio::spawn(async move {
            tracing::debug!("新的连接");
            let (mut reader, mut writer) = stream.into_split();
            let (tx, mut wx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
            let mut buf = vec![0u8; tg::g::DEFAULT_BUF_SIZE];

            let jhandler = tokio::spawn(async move {
                'wx_loop: loop {
                    let wbuf = match wx.recv().await {
                        Some(v) => v,
                        None => {
                            break 'wx_loop;
                        }
                    };

                    if let Err(err) = writer.write_all(&wbuf[..]).await {
                        tracing::error!("write failed: {err}");
                        break 'wx_loop;
                    }
                }
            });

            'read_loop: loop {
                let n = match reader.read(&mut buf).await {
                    Ok(0) => {
                        tracing::info!("EOF.");
                        break 'read_loop;
                    }
                    Ok(v) => v,
                    Err(err) => {
                        tracing::error!("{err}");
                        break 'read_loop;
                    }
                };

                let data = buf[..n].to_vec();
                if let Err(err) = tx.send(data) {
                    tracing::error!("tx.send faild: {err}");
                    break 'read_loop;
                }
            }

            jhandler.await.unwrap();
            tracing::debug!("连接断开");
        });
    }
}

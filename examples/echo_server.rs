use async_trait::async_trait;
use pack::WBUF_POOL;
use tg::{nw::pack::{self, RspBuf}, utils, make_wbuf};

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
    tg::tcp_server_run!("0.0.0.0:6688", 100, tg::g::DEFAULT_READ_TIMEOUT, EchoEvent);
}

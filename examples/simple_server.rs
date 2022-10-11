use async_trait::async_trait;
use tg::{nw::pack, utils};

#[derive(Clone, Copy, Default)]
struct SimpleEvent;

#[async_trait]
impl tg::nw::server::IEvent for SimpleEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &tg::nw::conn::ConnPtr<()>,
        req: &pack::Package,
    ) -> tg::g::Result<Option<pack::PackBuf>> {
        assert_eq!(req.idempotent(), conn.recv_seq());
        Ok(None)
    }

    async fn on_disconnected(&self, conn: &tg::nw::conn::ConnPtr<()>) {
        tracing::debug!(
            "[{}|{:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
        assert_eq!(10000, conn.recv_seq());
    }
}

#[tokio::main]
async fn main() {
    utils::init_log();
    tg::tcp_server_run!("0.0.0.0:6688", 100, tg::g::DEFAULT_READ_TIMEOUT, SimpleEvent);
}

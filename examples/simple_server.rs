use async_trait::async_trait;
use lockfree_object_pool::LinearReusable;
use tg::utils;

#[derive(Clone, Copy, Default)]
struct SimpleEvent;

#[async_trait]
impl tg::nw::server::IEvent for SimpleEvent {
    type U = ();

    async fn on_process(
        &self,
        _conn: &tg::nw::conn::ConnPtr<()>,
        _req: LinearReusable<'static, Vec<u8>>,
    ) -> tg::g::Result<()> {
        // assert_eq!(req.idempotent(), conn.recv_seq());
        Ok(())
    }

    async fn on_disconnected(&self, conn: &tg::nw::conn::ConnPtr<()>) {
        tracing::debug!(
            "[{}|{:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
        debug_assert_eq!(10000, conn.recv_seq());
    }
}

#[tokio::main]
async fn main() {
    utils::init_log();
    tg::tcp_server_run!("0.0.0.0:6688", 100, 10, SimpleEvent);
}

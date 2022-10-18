use async_trait::async_trait;
use tg::{nw::pack::{self, RSP_POOL}, utils};

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[derive(Clone, Default)]
struct EchoEvent;

#[async_trait]
impl tg::nw::server::IEvent for EchoEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &tg::nw::conn::ConnPtr<()>,
        req: &pack::Package,
    ) -> tg::g::Result<Option<pack::LinearItem>> {
        assert_eq!(req.idempotent(), conn.recv_seq());
        tracing::debug!("{}", req);
        let mut rsp = RSP_POOL.pull();
        rsp.set(req.package_id(), req.idempotent(), req.data());
        Ok(Some(rsp))
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

use async_trait::async_trait;
use lockfree_object_pool::LinearReusable;
use tg::utils;

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
        req: LinearReusable<'static, Vec<u8>>,
    ) -> tg::g::Result<()> {
        if let Err(err) = conn.send(req) {
            tracing::error!("on_process: {err}");
            Err(tg::g::Err::Custom("DOS".to_string()))
        } else {
            Ok(())
        }
    }

    async fn on_disconnected(&self, conn: &tg::nw::conn::ConnPtr<()>) {
        tracing::info!(
            "[{:?} - {:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.send_seq()
        );
    }
}

#[tokio::main]
async fn main() {
    utils::init_log();

    tracing::info!("EchoEvent size is {}", std::mem::size_of::<EchoEvent>());

    tg::tcp_server_run!("0.0.0.0:6688", 100, 0, EchoEvent);
}

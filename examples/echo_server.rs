use async_trait::async_trait;
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
        req: tg::nw::server::Pack,
    ) -> tg::g::Result<()> {
        // tracing::info!("{}", hex::encode(req.raw()));
        if let Err(err) = conn.send(req) {
            tracing::error!("on_process: {err}");
            Err(tg::g::Err::Custom("DOS".to_string()))
        } else {
            Ok(())
        }
    }

    #[allow(unused_variables)]
    async fn on_connected(&self, conn: &tg::nw::conn::ConnPtr<()>) -> tg::g::Result<()> {
        // tracing::info!("[{:?}] has connected", conn.remote());
        Ok(())
    }

    #[allow(unused_variables)]
    async fn on_disconnected(&self, conn: &tg::nw::conn::ConnPtr<()>) {
        tracing::info!("[{:?} - {:?}] has disconnected: {}", conn.sockfd(), conn.remote(), conn.send_seq());
    }
}

#[tokio::main]
async fn main() {
    utils::init_log();

    tracing::info!("EchoEvent size is {}", std::mem::size_of::<EchoEvent>());

    tg::tcp_server_run!("0.0.0.0:6688", 100, 0, EchoEvent);
}

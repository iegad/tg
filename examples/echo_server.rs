use std::{mem::MaybeUninit, sync::Once};

use async_trait::async_trait;
use bytes::Bytes;
use lockfree_object_pool::LinearObjectPool;
use tg::{nw::pack, utils};

#[derive(Clone, Default)]
struct EchoEvent;

#[async_trait]
impl tg::nw::IEvent for EchoEvent {
    type U = i32;

    async fn on_process(
        &self,
        conn: &tg::nw::Conn<i32>,
        req: &pack::Package,
    ) -> tg::g::Result<Option<Bytes>> {
        assert_eq!(req.idempotent(), conn.recv_seq());
        Ok(Some(req.to_bytes().freeze()))
    }

    async fn on_disconnected(&self, conn: &tg::nw::Conn<i32>) {
        tracing::debug!(
            "[{}|{:?}] has disconnected: {}",
            conn.sockfd(),
            conn.remote(),
            conn.recv_seq()
        );
    }

    fn conn_pool(&self) -> &LinearObjectPool<tg::nw::Conn<i32>> {
        static mut INSTANCE: MaybeUninit<LinearObjectPool<tg::nw::Conn<i32>>> =
            MaybeUninit::uninit();
        static ONCE: Once = Once::new();

        ONCE.call_once(|| unsafe {
            INSTANCE
                .as_mut_ptr()
                .write(LinearObjectPool::new(|| tg::nw::Conn::new(), |v| v.reset()));
        });

        unsafe { &*INSTANCE.as_ptr() }
    }
}

#[async_trait]
impl tg::nw::IServerEvent for EchoEvent {}

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let server = tg::nw::Server::<EchoEvent>::new("0.0.0.0:6688", 100, tg::g::DEFAULT_READ_TIMEOUT);
    if let Err(err) = tg::nw::tcp::server_run(server.clone()).await {
        println!("{:?}", err);
    }
}

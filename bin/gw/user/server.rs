use async_trait::async_trait;
use bytes::Bytes;
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use std::{mem::MaybeUninit, sync::Once};
use tg::{
    g,
    nw::{self, pack},
};

lazy_static! {
    static ref CONN_POOL: LinearObjectPool<tg::nw::Conn<()>> = tg::nw::Conn::<()>::pool();
}

#[derive(Clone, Copy, Default)]
pub struct UserEvent;

#[async_trait]
impl nw::IEvent for UserEvent {
    type U = ();

    async fn on_process(
        &self,
        _conn: &tg::nw::Conn<()>,
        req: &pack::Package,
    ) -> tg::g::Result<Option<Bytes>> {
        // tracing::debug!("[{:?}] => {:?}", conn.remote(), req);
        Ok(Some(req.to_bytes().freeze()))
    }
}

#[async_trait]
impl nw::IServerEvent for UserEvent {}

pub struct UserServer {
    server: nw::ServerPtr<UserEvent>,
}

impl UserServer {
    fn new(host: &'static str, max_connections: usize, timeout: u64) -> Self {
        Self {
            server: nw::Server::new_ptr(host, max_connections, timeout),
            // sessions: vec![0u32; max_connections],
        }
    }

    pub fn run(server: nw::ServerPtr<UserEvent>) {
        let server_copy = server.clone();
        tokio::spawn(async move {
            if let Err(err) = nw::tcp::server_run(server_copy, &CONN_POOL).await {
                tracing::error!("{:?}", err);
            }
        });
    }

    pub fn stop(server: nw::ServerPtr<UserEvent>) {
        server.shutdown();
    }

    pub fn instance() -> &'static UserServer {
        static mut INSTANCE: MaybeUninit<UserServer> = MaybeUninit::uninit();
        static ONCE: Once = Once::new();

        ONCE.call_once(|| unsafe {
            INSTANCE.as_mut_ptr().write(UserServer::new(
                "0.0.0.0:6688",
                g::DEFAULT_MAX_CONNECTIONS,
                g::DEFAULT_READ_TIMEOUT,
            ));
        });

        unsafe { &*INSTANCE.as_ptr() }
    }

    pub fn server(&self) -> nw::ServerPtr<UserEvent> {
        self.server.clone()
    }
}

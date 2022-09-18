use async_trait::async_trait;
use bytes::Bytes;
use std::{mem::MaybeUninit, sync::Once};
use tg::{
    g,
    nw::{self, pack},
};

#[derive(Clone, Copy, Default)]
pub struct UserEvent;

#[async_trait]
impl nw::IEvent for UserEvent {
    async fn on_process(
        &self,
        _conn: &tg::nw::Conn,
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
            server: nw::Server::new(host, max_connections, timeout),
            // sessions: vec![0u32; max_connections],
        }
    }

    pub fn run(server: nw::ServerPtr<UserEvent>) {
        let server_copy = server.clone();
        tokio::spawn(async move {
            if let Err(err) = nw::tcp::server_run(server_copy).await {
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

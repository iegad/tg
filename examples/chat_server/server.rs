use async_trait::async_trait;
use hashbrown::HashMap;
use lazy_static::lazy_static;
use tg::make_wbuf;
use tg::nw::conn::{ConnPool, Conn};
use tg::nw::pack::RspBuf;
use tg::nw::server::Server;
use std::sync::{Arc, Mutex};
use tg::{g, nw::pack::WBUF_POOL};

type ConnPtr = tg::nw::conn::ConnPtr<()>;

lazy_static! {
    pub static ref SERVER: Arc<Server<ChatEvent>> = Server::new_arc("0.0.0.0:6688", g::DEFAULT_MAX_CONNECTIONS, 0);
    pub static ref SESSIONS: Arc<Mutex<HashMap<tg::nw::Socket, ConnPtr>>> = Arc::new(Mutex::new(HashMap::new()));
    pub static ref CONN_POOL: ConnPool<()> = Conn::pool();
}

#[derive(Clone, Default)]
pub struct ChatEvent;

#[async_trait]
impl tg::nw::server::IEvent for ChatEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &ConnPtr,
        req: &tg::nw::pack::Package,
    ) -> g::Result<RspBuf> {
        assert_eq!(req.idempotent(), conn.recv_seq());

        tracing::info!("[{:?}] {req}", conn.remote());
        let mut wbuf = WBUF_POOL.pull();
        req.to_bytes(&mut wbuf);
        Ok(make_wbuf!(wbuf))
    }

    async fn on_connected(&self, conn: &ConnPtr) -> g::Result<()> {
        SESSIONS.lock().unwrap().insert(conn.sockfd(), conn.clone());
        tracing::debug!("[{}] has connected", conn.clone().sockfd());
        Ok(())
    }

    async fn on_disconnected(&self, conn: &ConnPtr) {
        SESSIONS.lock().unwrap().remove(&conn.sockfd());
        tracing::debug!("[{}] has disconnected", conn.sockfd());
    }
}

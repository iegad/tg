use async_trait::async_trait;
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tg::{g, nw::pack::WBUF_POOL};

type ConnPtr = tg::nw::ConnPtr<()>;

lazy_static! {
    pub static ref SERVER: tg::nw::ServerPtr<ChatEvent> = tg::nw::Server::new_ptr("0.0.0.0:6688", g::DEFAULT_MAX_CONNECTIONS, g::DEFAULT_READ_TIMEOUT);
    pub static ref SESSIONS: Arc<Mutex<HashMap<u64, ConnPtr>>> = Arc::new(Mutex::new(HashMap::new()));
    pub static ref CONN_POOL: LinearObjectPool<tg::nw::Conn<()>> = tg::nw::Conn::pool();
}

#[derive(Clone, Default)]
pub struct ChatEvent;

#[async_trait]
impl tg::nw::IServerEvent for ChatEvent {
    type U = ();

    async fn on_process(
        &self,
        conn: &ConnPtr,
        req: &tg::nw::pack::Package,
    ) -> g::Result<Option<tg::nw::pack::Response>> {
        assert_eq!(req.idempotent(), conn.recv_seq());

        let mut wbuf = WBUF_POOL.pull();
        req.to_bytes(&mut wbuf);
        Ok(Some(Arc::new(wbuf)))
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

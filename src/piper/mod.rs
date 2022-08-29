use crate::{
    g,
    nw::{
        self,
        conn::Conn,
        pack::{PACK_POOL, RSP_POOL},
        tcp, IProc,
    },
};
use async_trait::async_trait;
use bytes::BytesMut;
use lazy_static::lazy_static;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tokio::{io::AsyncWriteExt, net::TcpStream};

lazy_static! {
    static ref SERV_HANDLERS: Arc<RwLock<HashMap<u16, Arc<dyn Handler>>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

#[async_trait]
pub trait Handler: Sync + Send + 'static {
    fn request_id(&self) -> u16;
    fn response_id(&self) -> u16;
    fn remark(&self) -> &str;
    async fn handle(&self, conn: &Conn, req: &[u8]) -> BytesMut;
}

#[derive(Clone, Copy)]
pub struct Piper;

#[async_trait]
impl IProc for Piper {
    async fn on_process(
        &self,
        conn: &nw::conn::Conn,
        req: &nw::pack::Package,
    ) -> g::Result<BytesMut> {
        let handler = match SERV_HANDLERS.read().unwrap().get(&req.package_id()) {
            None => return Err(g::Err::PiperPIDInvalid),
            Some(v) => v.clone(),
        };

        Ok(handler.handle(conn, req.data()).await)
    }
}

pub fn regist(handler: Arc<dyn Handler>) -> g::Result<()> {
    let mut map = SERV_HANDLERS.write().unwrap();
    if map.contains_key(&handler.request_id()) {
        return Err(g::Err::PiperHandlerAreadyExists(handler.request_id()));
    }

    map.insert(handler.request_id(), handler);
    Ok(())
}

pub async fn call(
    sock: &mut TcpStream,
    service_id: u16,
    package_id: u16,
    router_id: u32,
    idempotent: u32,
    token: u32,
    data: &[u8],
) -> g::Result<Option<BytesMut>> {
    let mut req = PACK_POOL.pull();
    req.set_service_id(service_id);
    req.set_package_id(package_id);
    req.set_router_id(router_id);
    req.set_idempotent(idempotent);
    req.set_token(token);
    req.set_data(data);

    if let Err(err) = sock.write_all_buf(&mut req.to_bytes()).await {
        return Err(g::Err::TcpWriteFailed(format!("{:?}", err)));
    }

    let mut rsp = RSP_POOL.pull();
    match tcp::read(sock, &mut rsp).await {
        Ok(0) => return Ok(None),
        Ok(_) => return Ok(Some(rsp.to_bytes())),
        Err(err) => return Err(err),
    }
}

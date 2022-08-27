use async_trait::async_trait;
use bytes::BytesMut;
use std::sync::Arc;
use tg::{
    g,
    nw::{self, conn::Conn, pack::PACK_POOL, tcp},
    piper::{self, Handler},
};

#[derive(Clone, Copy)]
struct Echo;

#[async_trait]
impl Handler for Echo {
    fn request_id(&self) -> u16 {
        1
    }

    fn response_id(&self) -> u16 {
        2
    }

    fn remark(&self) -> &str {
        "1234567890"
    }

    async fn handle(&self, conn: &Conn, req: &[u8]) -> BytesMut {
        println!("CONN[{}|{:?}] => {:?}", conn.sockfd(), conn.remote(), req);
        let mut rsp = PACK_POOL.pull();
        rsp.set_service_id(1);
        rsp.set_package_id(2);
        rsp.set_data(req);
        rsp.to_bytes().clone()
    }
}

unsafe impl Send for Echo {}
unsafe impl Sync for Echo {}

#[derive(Clone, Copy)]
struct Hello;

#[async_trait]
impl Handler for Hello {
    fn request_id(&self) -> u16 {
        3
    }

    fn response_id(&self) -> u16 {
        4
    }

    fn remark(&self) -> &str {
        "1234567890"
    }

    async fn handle(&self, conn: &Conn, req: &[u8]) -> BytesMut {
        println!("CONN[{}|{:?}] => {:?}", conn.sockfd(), conn.remote(), req);
        BytesMut::new()
    }
}

unsafe impl Send for Hello {}
unsafe impl Sync for Hello {}

#[tokio::main]
async fn main() -> g::Result<()> {
    let server = match nw::Server::new("0.0.0.0:6688", 100) {
        Err(err) => panic!("{}", err),
        Ok(s) => s,
    };

    piper::regist(Arc::new(Hello {})).unwrap();
    piper::regist(Arc::new(Echo {})).unwrap();

    tcp::run(&server, piper::Piper {}).await;
    Ok(())
}

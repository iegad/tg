use async_trait::async_trait;
use bytes::BytesMut;
use tg::nw::ws;
use tg::{
    g,
    nw::{self},
};

#[derive(Clone, Copy)]
struct EchoProc;

#[async_trait]
impl nw::ISvrProc for EchoProc {
    async fn on_process(
        &self,
        _conn: &nw::conn::Conn,
        req: &nw::pack::Package,
    ) -> g::Result<Option<BytesMut>> {
        // println!(
        //     "from conn[{}:{:?}] => idempotent: {}, {}",
        //     conn.sockfd(),
        //     conn.remote(),
        //     req.idempotent(),
        //     unsafe { std::str::from_utf8_unchecked(req.data()) }
        // );

        let wbuf = req.to_bytes();
        Ok(Some(wbuf))
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let server = match nw::Server::new("0.0.0.0:6688", 100) {
        Err(err) => panic!("{}", err),
        Ok(s) => s,
    };

    Ok(ws::server_run(&server, EchoProc {}).await)
}

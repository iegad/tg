use async_trait::async_trait;
use bytes::BytesMut;
use tg::nw::tcp;
use tg::{
    g,
    nw::{self},
};

#[derive(Clone, Copy)]
struct EchoProc;

#[async_trait]
impl nw::IProc for EchoProc {
    async fn on_process(
        &self,
        _conn: &nw::conn::Conn,
        req: &nw::pack::Package,
    ) -> g::Result<BytesMut> {
        // println!(
        //     "[{}]from conn[{}:{:?}] => idempotent: {}, {}",
        //     thread_id::get(),
        //     conn.sockfd(),
        //     conn.remote(),
        //     req.idempotent(),
        //     unsafe { std::str::from_utf8_unchecked(req.data()) }
        // );

        let wbuf = req.to_bytes();
        Ok(wbuf)
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let server = match nw::Server::new("0.0.0.0:6688", 100) {
        Err(err) => panic!("{}", err),
        Ok(s) => s,
    };

    Ok(tcp::run(&server, EchoProc {}).await)
}

use tg::{
    g,
    nw::{self, iface, tcp::Server},
};
use tokio::sync::broadcast;

#[derive(Clone, Copy)]
struct EchoProc;

impl nw::iface::IProc for EchoProc {
    fn on_process(&self, conn: &dyn iface::IConn, req: &nw::pack::Package) -> g::Result<Vec<u8>> {
        println!(
            "[{}]from conn[{}:{:?}] => {}",
            thread_id::get(),
            conn.sockfd(),
            conn.remote(),
            unsafe { std::str::from_utf8_unchecked(req.data()) }
        );

        Ok(req.to_bytes())
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let server = match Server::new("0.0.0.0:6688", 100) {
        Err(err) => panic!("{}", err),
        Ok(s) => s,
    };

    let (tx, _) = broadcast::channel(1);
    Ok(server.run(EchoProc {}, tx).await)
}

use std::sync::Arc;

use tg::{
    g,
    nw::{self, iface, pack::Package},
};

#[derive(Clone, Copy)]
struct EchoProc;

impl nw::iface::IProc for EchoProc {
    fn on_process(&self, conn: &mut dyn iface::IConn) -> g::Result<Arc<Package>> {
        let req = conn.req();

        println!(
            "[{}]from conn[{}:{:?}] => {}",
            thread_id::get(),
            conn.sockfd(),
            conn.remote(),
            // unsafe { std::str::from_utf8_unchecked(req.data()) }
            req.data_len(),
        );

        Ok(Arc::new(Package::with_params(
            req.pid(),
            req.idempotent(),
            req.data(),
        )))
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let server = nw::tcp::Server::new("0.0.0.0:6688", 100)?;
    Ok(server.run(EchoProc {}).await)
}

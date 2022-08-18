use tg::{
    g,
    nw::{self, iface, pack},
};

#[derive(Clone, Copy)]
struct EchoProc;

impl nw::iface::IProc for EchoProc {
    fn on_process(&self, conn: &mut dyn iface::IConn) -> g::Result<pack::PackagePtr> {
        let req = conn.req();

        // println!(
        //     "[{}]from conn[{}:{:?}] => {}",
        //     thread_id::get(),
        //     conn.sockfd(),
        //     conn.remote(),
        //     unsafe { std::str::from_utf8_unchecked(req.data()) } // req.data_len(),
        // );

        let mut rsp = pack::PACK_POOL.pull();
        rsp.set_pid(req.pid());
        rsp.set_idempotent(req.idempotent());
        rsp.set_data(req.data());

        Ok(rsp)
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let server = nw::tcp::Server::new("0.0.0.0:6688", 100)?;
    Ok(server.run(EchoProc {}).await)
}

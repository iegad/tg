use tg::{
    g,
    nw::{self, pack},
};

#[derive(Clone, Copy)]
struct EchoProc;

impl nw::IProc for EchoProc {
    fn on_process(&self, conn: &nw::Conn) -> g::Result<pack::PackageItem> {
        let req = conn.req();

        if !conn.remote().ip().to_string().starts_with("127") {
            println!(
                "[pid: {}; idempotent: {}; raw_len: {}; data: {}]",
                req.service_id(),
                req.idempotent(),
                req.raw_len(),
                core::str::from_utf8(req.data()).unwrap()
            );
        }

        let mut rsp = pack::PACK_POOL.pull();
        rsp.set_service_id(req.service_id());
        rsp.set_router_id(req.router_id());
        rsp.set_token(req.token());
        rsp.set_package_id(req.package_id());
        rsp.set_idempotent(req.idempotent());
        rsp.set_data(req.data());
        Ok(rsp)
    }
}

#[tokio::main]
async fn main() -> g::Result<()> {
    let server = nw::Server::new("0.0.0.0:6688", 100)?;
    Ok(nw::tcp::run(&server, EchoProc {}).await)
}

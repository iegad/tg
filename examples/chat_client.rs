use tg::nw::{packet::REQ_POOL, conn};

#[derive(Clone, Default)]
struct EchoEvent;

#[tokio::main]
async fn main() {
    tg::utils::init_log();

    let gp = conn::Group::new_arc("127.0.0.1:6688", 1);
    let gpc = gp.clone();
    let j = tokio::spawn(async move {
        tg::nw::tcp::group_run(gpc).await;
    });
    
    for i in 0..10 {
        let mut pkt = REQ_POOL.pull();
        pkt.set(1, i + 1, "Hello world".as_bytes());
        gp.send(pkt).await.unwrap();
    }

    j.await.unwrap();
}
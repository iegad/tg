use bytes::{BufMut, BytesMut};
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc,
};
use tokio::sync::broadcast::{self, Receiver, Sender};

static NCOUNT: AtomicI32 = AtomicI32::new(0);
static DCOUNT: AtomicI32 = AtomicI32::new(0);

lazy_static::lazy_static! {
    static ref POOL: LinearObjectPool<BytesMut> =
        LinearObjectPool::new(|| {NCOUNT.fetch_add(1, Ordering::SeqCst) ; BytesMut::new()}, |v| {v.clear(); DCOUNT.fetch_add(1, Ordering::SeqCst);});
}

#[tokio::main]
async fn main() {
    let (tx, mut rx): (
        Sender<Arc<LinearReusable<BytesMut>>>,
        Receiver<Arc<LinearReusable<BytesMut>>>,
    ) = broadcast::channel(10);

    tokio::spawn(async move {
        for _ in 0..1000 {
            let v = rx.recv().await.unwrap();
            println!("recv: {:?}", &v[..]);
        }
    });

    for _ in 0..1000 {
        let mut data = POOL.pull();
        data.put("Hello".as_bytes());

        let v = Arc::new(data);

        if let Err(_) = tx.send(v.clone()) {
            println!("send failed");
        }

        tokio::time::sleep(std::time::Duration::from_micros(10)).await;
    }

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    println!(
        "new: {}, delete: {}",
        NCOUNT.load(Ordering::SeqCst),
        DCOUNT.load(Ordering::SeqCst)
    );
}

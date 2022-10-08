use futures_util::future::join_all;
use tg::{g, utils};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    utils::init_log();

    // tonic_build::compile_protos("src/pb/lisa.proto")?;

    let mut arr = Vec::new();
    let (tx, rx) = async_channel::bounded(g::DEFAULT_CHAN_SIZE);

    for _ in 0..5 {
        let rx_c = rx.clone();
        let t = tokio::spawn(async move {
            for _ in 0..10 {
                let data = rx_c.recv().await.unwrap();
                tracing::debug!("data: {}", data);
            }
        });
        arr.push(t);
    }

    for i in 0..50 {
        tx.send(i).await.unwrap();
    }

    join_all(arr).await;
    Ok(())
}

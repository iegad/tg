use async_trait::async_trait;
use bytes::BytesMut;
use tg::{g, nw::pack, utils};
use tokio::{
    io::{AsyncBufReadExt, BufReader, AsyncWriteExt, AsyncReadExt},
    net::TcpStream,
    sync::broadcast, select,
};

#[derive(Default, Clone, Copy)]
struct ChatEvent;

#[tokio::main]
async fn main() {
    utils::init_log(tracing::Level::DEBUG);

    let mut reader = BufReader::new(tokio::io::stdin());
    let mut line = String::new();
    let (tx, _) = broadcast::channel(1);
    let shutdown_tx = tx.clone();

    'stdin_loop: loop {
        let result_read = reader.read_line(&mut line).await;
        if let Err(err) = result_read {
            tracing::error!("{:?}", err);
            break 'stdin_loop;
        }

        let data = line.trim();
        if data.to_ascii_lowercase() == "exit" {
            tracing::debug!("...");
            tx.send(1).unwrap();
            break 'stdin_loop;
        }

        line.clear();
    }

    tracing::debug!("[EXIT].");
}

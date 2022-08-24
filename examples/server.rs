use async_cell::sync::AsyncCell;
use async_trait::async_trait;
use bytes::BytesMut;
use lazy_static::lazy_static;
use std::{net::SocketAddr, os::unix::prelude::AsRawFd, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select,
    sync::broadcast,
};

type Response = BytesMut;

lazy_static! {
    static ref PACK_POOL: async_object_pool::Pool<BytesMut> = async_object_pool::Pool::new(100000);
    static ref CONN_MAP: Arc<cht::HashMap<i32, Arc<Conn>>> =
        Arc::new(cht::HashMap::with_capacity(100));
}

#[derive(Debug)]
struct Conn {
    connfd: AsyncCell<i32>,
    tx: broadcast::Sender<Response>,
    remote: AsyncCell<SocketAddr>,
    local: AsyncCell<SocketAddr>,
}

impl Conn {
    fn new(stream: &TcpStream) -> Self {
        let (tx, _) = broadcast::channel(100);

        Self {
            connfd: AsyncCell::new_with(stream.as_raw_fd()),
            tx,
            remote: AsyncCell::new_with(stream.peer_addr().unwrap()),
            local: AsyncCell::new_with(stream.local_addr().unwrap()),
        }
    }

    fn load(&self, stream: &TcpStream) {
        self.remote.set(stream.peer_addr().unwrap());
        self.local.set(stream.local_addr().unwrap());
        self.connfd.set(stream.as_raw_fd());
    }

    async fn connfd(&self) -> i32 {
        self.connfd.get().await
    }

    async fn valid(&self) -> bool {
        self.connfd.get().await > 0
    }

    async fn remote(&self) -> SocketAddr {
        self.remote.get().await
    }

    async fn _local(&self) -> SocketAddr {
        self.local.get().await
    }

    fn sender(&self) -> broadcast::Sender<Response> {
        self.tx.clone()
    }

    fn receiver(&self) -> broadcast::Receiver<Response> {
        self.tx.subscribe()
    }
}

#[async_trait]
trait Event: Send + Sync + Copy + 'static {
    async fn on_init(&self) -> bool {
        println!("server is running");
        true
    }

    async fn on_released(&self) {
        println!("server has stopped");
    }

    async fn on_connected(&self, conn: &Conn) {
        println!(
            "[{}|{:?}] has connected",
            conn.connfd().await,
            conn.remote().await,
        );
    }

    async fn on_disconnected(&self, conn: &Conn) {
        println!(
            "[{}|{:?}] has disconnected",
            conn.connfd().await,
            conn.remote().await,
        );
    }

    async fn on_message(&self, conn: &Conn, req: &BytesMut, n: usize);
}

struct Server<T> {
    event: T,
}

impl<T: Event> Server<T> {
    fn new(event: T) -> Self {
        Self { event }
    }

    async fn run(&self, listener: TcpListener) {
        self.event.on_init().await;

        let event = self.event;
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                Self::conn_handle(stream, event).await;
            });
        }
    }

    async fn conn_handle(mut stream: TcpStream, event: T) {
        let connfd = stream.as_raw_fd();

        let map = CONN_MAP.clone();
        if let None = map.get(&connfd) {
            println!("add conn [{}]", connfd);
            map.insert(connfd, Arc::new(Conn::new(&stream)));
        }

        let conn = &mut map.get(&connfd).unwrap();
        conn.load(&stream);

        let (mut reader, mut writer) = stream.split();
        let mut rx = conn.receiver();

        let mut rbuf = PACK_POOL
            .take_or_create(|| BytesMut::with_capacity(4096))
            .await;

        event.on_connected(&conn).await;

        loop {
            select! {
                result_read = reader.read_buf(&mut rbuf) => {
                    let n = match result_read {
                        Ok(0) => {
                            break;
                        }

                        Ok(n) => n,

                        Err(err) => {
                            println!("read failed: {:?}", err);
                            break;
                        }
                    };

                    event.on_message(&conn, &rbuf, n).await;
                    rbuf.clear();
                }

                result_rx = rx.recv() => {
                    if let Ok(v) = result_rx {
                        if let Err(err) = writer.write_all(&v[..v.len()]).await {
                            println!("write failed: {:?}", err);
                            break;
                        }
                    }
                }
            }
        }

        event.on_disconnected(&conn).await;
        PACK_POOL.put(rbuf).await;
        map.remove(&connfd);
    }
}

#[derive(Copy, Clone)]
pub struct Echo;

#[async_trait]
impl Event for Echo {
    async fn on_message(&self, conn: &Conn, req: &BytesMut, n: usize) {
        println!(
            "[{}|{}] => {}",
            conn.connfd().await,
            conn.remote().await,
            core::str::from_utf8(&req[..n]).unwrap()
        );

        let map = CONN_MAP.clone();
        let n = map.len() + 10;

        println!(">>> map len: {}", n - 10);
        for i in 10..n {
            if let Some(c) = &map.get(&(i as i32)) {
                if c.valid().await {
                    let tx = c.sender();
                    tx.send(req.clone()).unwrap();
                }
                println!("conn[{}] => {}", c.connfd().await, c.valid().await);
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:8888").await.unwrap();
    let server = Server::new(Echo {});
    server.run(listener).await;
}

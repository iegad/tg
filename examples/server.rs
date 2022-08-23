use async_trait::async_trait;
use bytes::BytesMut;
use lazy_static::lazy_static;
use std::{collections::HashMap, net::SocketAddr, os::unix::prelude::AsRawFd, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select,
    sync::{mpsc, RwLock},
};

type Response = BytesMut;

lazy_static! {
    static ref CWCH_MAP: Arc<RwLock<HashMap<i32, mpsc::Sender<Response>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    static ref PACK_POOL: async_object_pool::Pool<BytesMut> = async_object_pool::Pool::new(100000);
    static ref CONN_POOL: async_object_pool::Pool<Conn> = async_object_pool::Pool::new(10000);
}

struct Conn {
    stream: Option<TcpStream>,
    tx: Option<mpsc::Sender<Response>>,
    remote: Option<SocketAddr>,
    local: Option<SocketAddr>,
}

impl Conn {
    fn new() -> Self {
        Self {
            stream: None,
            tx: None,
            remote: None,
            local: None,
        }
    }

    fn load(&mut self, stream: TcpStream, tx: mpsc::Sender<Response>) {
        self.remote = Some(stream.peer_addr().unwrap());
        self.local = Some(stream.local_addr().unwrap());
        self.stream = Some(stream);
        self.tx = Some(tx);
    }

    fn connfd(&self) -> i32 {
        if let Some(s) = self.stream.as_ref() {
            return s.as_raw_fd();
        }
        -1
    }

    fn _valid(&self) -> bool {
        self.stream.is_some() && self.tx.is_some()
    }

    fn release(&mut self) {
        if let Some(s) = self.stream.as_ref() {
            drop(s);
            self.stream = None;
        }

        if let Some(tx) = self.tx.as_ref() {
            drop(tx);
            self.tx = None;
        }
    }

    fn remote(&self) -> &SocketAddr {
        &self.remote.as_ref().unwrap()
    }

    // fn local(&self) -> &SocketAddr {
    //     &self.local.as_ref().unwrap()
    // }

    fn stream_mut(&mut self) -> &mut TcpStream {
        self.stream.as_mut().unwrap()
    }

    // async fn send(&self, rsp: Response) {
    //     if let Some(tx) = self.tx.clone() {
    //         if let Err(err) = tx.send(rsp).await {
    //             panic!("wch send failed: {:?}", err);
    //         }
    //     }
    // }

    fn new_sender(&mut self) -> mpsc::Sender<Response> {
        self.tx.as_ref().unwrap().clone()
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
        println!("[{}|{:?}] has connected", conn.connfd(), conn.remote(),);
    }

    async fn on_disconnected(&self, conn: &Conn) {
        println!("[{}|{:?}] has disconnected", conn.connfd(), conn.remote());
    }

    async fn on_message(&self, conn: &mut Conn, req: &BytesMut, n: usize);
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
            let (conn, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                Self::conn_handle(conn, event).await;
            });
        }
    }

    async fn conn_handle(stream: TcpStream, event: T) {
        let (tx, mut rx) = mpsc::channel::<Response>(100);

        let mut conn = CONN_POOL.take_or_create(|| Conn::new()).await;
        conn.load(stream, tx);
        let txc = conn.new_sender();

        CWCH_MAP.write().await.insert(conn.connfd(), txc);
        let mut rbuf = PACK_POOL
            .take_or_create(|| BytesMut::with_capacity(4096))
            .await;

        event.on_connected(&conn).await;

        loop {
            select! {
                result_read = conn.stream_mut().read_buf(&mut rbuf) => {
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

                    event.on_message(&mut conn, &rbuf, n).await;
                    rbuf.clear();
                }

                result_rx = rx.recv() => {
                    if let Some(v) = result_rx {
                        if let Err(err) = conn.stream_mut().write_all(&v[..v.len()]).await {
                            println!("write failed: {:?}", err);
                            break;
                        }
                    }
                }
            }
        }

        event.on_disconnected(&conn).await;

        PACK_POOL.put(rbuf).await;
        conn.release();
        CONN_POOL.put(conn).await;
    }
}

#[derive(Copy, Clone)]
pub struct Echo;

#[async_trait]
impl Event for Echo {
    async fn on_message(&self, conn: &mut Conn, req: &BytesMut, n: usize) {
        println!(
            "[{}|{}] => {}",
            conn.connfd(),
            conn.remote(),
            core::str::from_utf8(&req[..n]).unwrap()
        );

        let map = &CWCH_MAP.read().await.to_owned();

        let data = req.clone();
        for (connfd, sender) in map {
            println!(
                "send data to => [{}] => {}",
                connfd,
                core::str::from_utf8(req).unwrap()
            );

            if let Err(err) = sender.send(data.clone()).await {
                panic!("{:?}", err);
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

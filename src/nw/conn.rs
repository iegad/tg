use crate::g;
use bytes::BytesMut;
use lockfree_object_pool::LinearObjectPool;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::prelude::AsRawFd,
};
use tokio::{net::TcpStream, sync::broadcast};

lazy_static::lazy_static! {
    pub static ref CONN_POOL: LinearObjectPool<Conn> = LinearObjectPool::new(||Conn::new(), |v|{v.reset();});
}

pub struct Conn {
    sockfd: i32,
    send_seq: u32,
    recv_seq: u32,
    tx: broadcast::Sender<BytesMut>,
    remote: SocketAddr,
    local: SocketAddr,
    rbuf: BytesMut,
}

impl Conn {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);

        Self {
            sockfd: 0,
            send_seq: 0,
            recv_seq: 0,
            tx,
            remote: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            local: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            rbuf: BytesMut::new(),
        }
    }

    pub fn with_new(stream: &TcpStream) -> Self {
        let (tx, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);

        Self {
            sockfd: stream.as_raw_fd(),
            send_seq: 0,
            recv_seq: 0,
            tx,
            remote: stream.peer_addr().unwrap(),
            local: stream.local_addr().unwrap(),
            rbuf: BytesMut::new(),
        }
    }

    pub fn from_with(&mut self, stream: &TcpStream) {
        self.sockfd = stream.as_raw_fd();
        self.remote = stream.peer_addr().unwrap();
        self.local = stream.local_addr().unwrap();
    }

    pub fn sender(&self) -> broadcast::Sender<BytesMut> {
        self.tx.clone()
    }

    pub fn receiver(&self) -> broadcast::Receiver<BytesMut> {
        self.tx.subscribe()
    }

    pub fn valid(&self) -> bool {
        self.sockfd > 0
    }

    pub fn sockfd(&self) -> i32 {
        self.sockfd
    }

    pub fn rbuf_mut(&mut self) -> &mut BytesMut {
        &mut self.rbuf
    }

    pub fn rbuf(&self) -> &BytesMut {
        &self.rbuf
    }

    pub fn remote(&self) -> &SocketAddr {
        &self.remote
    }

    pub fn local(&self) -> &SocketAddr {
        &self.local
    }

    pub fn reset(&mut self) {
        self.sockfd = 0;
    }

    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    pub fn send_seq_incr(&mut self) {
        self.send_seq += 1;
    }

    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    pub fn recv_seq_incr(&mut self) {
        self.recv_seq += 1;
    }
}

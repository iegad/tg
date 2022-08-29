use crate::g;
use bytes::BytesMut;
use lockfree_object_pool::LinearObjectPool;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::prelude::AsRawFd,
};
use tokio::{net::TcpStream, sync::broadcast};

lazy_static::lazy_static! {
    /// CONN_POOL对象池, 用于创建 conn::Conn实例
    pub static ref CONN_POOL: LinearObjectPool<Conn> = LinearObjectPool::new(||Conn::new(), |v|{v.reset();});
}

/// 连接会话, 用于 server端
pub struct Conn {
    sockfd: i32,                     // 套接字描述符
    send_seq: u32,                   // 发送序列
    recv_seq: u32,                   // 接收序列
    tx: broadcast::Sender<BytesMut>, // 消息发送管道的消息生产者
    remote: SocketAddr,              // 远端地址
    local: SocketAddr,               // 本端地址
    rbuf: BytesMut,                  // 读缓冲区
}

impl Conn {
    /// 创建一个空的 Conn实例, 用于CONN_POOL中使用.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);

        Self {
            sockfd: 0,
            send_seq: 0,
            recv_seq: 0,
            tx,
            remote: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            local: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            rbuf: BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        }
    }

    /// 通过TcpStream 创建 Conn实例
    pub fn with_new(stream: &TcpStream) -> Self {
        let (tx, _) = broadcast::channel(g::DEFAULT_CHAN_SIZE);

        Self {
            sockfd: stream.as_raw_fd(),
            send_seq: 0,
            recv_seq: 0,
            tx,
            remote: stream.peer_addr().unwrap(),
            local: stream.local_addr().unwrap(),
            rbuf: BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        }
    }

    /// 通过 TcpStream 初始化自身
    pub fn init(&mut self, stream: &TcpStream) {
        self.sockfd = stream.as_raw_fd();
        self.remote = stream.peer_addr().unwrap();
        self.local = stream.local_addr().unwrap();
    }

    /// 获取 发送管道
    pub fn sender(&self) -> broadcast::Sender<BytesMut> {
        self.tx.clone()
    }

    /// 获取 接收管道
    pub fn receiver(&self) -> broadcast::Receiver<BytesMut> {
        self.tx.subscribe()
    }

    /// 判断该 Conn是否有效
    pub fn valid(&self) -> bool {
        self.sockfd > 0
    }

    /// 获取原生 套接字描述符
    pub fn sockfd(&self) -> i32 {
        self.sockfd
    }

    /// 获取Conn 读缓冲区 mut
    pub fn rbuf_mut(&mut self) -> &mut BytesMut {
        &mut self.rbuf
    }

    /// 获取Conn 读缓冲区
    pub fn rbuf(&self) -> &BytesMut {
        &self.rbuf
    }

    /// 获取远端地址
    pub fn remote(&self) -> &SocketAddr {
        &self.remote
    }

    /// 获取本端地址
    pub fn local(&self) -> &SocketAddr {
        &self.local
    }

    /// 重置 Conn实例
    pub fn reset(&mut self) {
        self.sockfd = 0;
    }

    /// 获取发送序列
    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    /// 发送序列递增
    pub fn send_seq_incr(&mut self) {
        self.send_seq += 1;
    }

    /// 获取接收序列
    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    /// 接收序列递增
    pub fn recv_seq_incr(&mut self) {
        self.recv_seq += 1;
    }
}

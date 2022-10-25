#[cfg(unix)]
use std::os::unix::prelude::AsRawFd;
#[cfg(windows)]
use std::os::windows::prelude::AsRawSocket;
use std::{net::SocketAddr, sync::Arc};
use async_trait::async_trait;
use tokio::{net::tcp::OwnedReadHalf, sync::broadcast};
use crate::g;
use super::{packet, server, Socket};

pub type Ptr<'a, T> = Arc<Client<'a, T>>;

#[async_trait]
pub trait IEvent: Clone + Default + Send + Sync + 'static {
    type U: Default + Send + Sync + 'static;

    async fn on_process(&self, cli: &Ptr<Self::U>, pkt: server::Packet) -> g::Result<()>;

    /// 会话端错误事件
    ///
    /// # 触发
    ///
    /// 在会话端会读写出现错误时触发.
    async fn on_error(&self, cli: &Ptr<Self::U>, err: g::Err) {
        tracing::error!("[{:?} - {:?}] => {err}", cli.sockfd(), cli.remote());
    }

    /// 会话端连接成功事件
    ///
    /// # 触发
    ///
    /// 在会话端连接连接成功后触发.
    async fn on_connected(&self, cli: &Ptr<Self::U>) -> g::Result<()> {
        tracing::debug!("[{:?} - {:?}] has connected", cli.sockfd(), cli.remote());
        Ok(())
    }

    /// 会话端连接断开事件
    ///
    /// # 触发
    ///
    /// 在会话端连接断开后触发.
    async fn on_disconnected(&self, cli: &Ptr<Self::U>) {
        tracing::debug!("[{:?} - {:?}] has disconnected", cli.sockfd(), cli.remote());
    }
}

pub struct Client<'a, U: Default + Send + Sync + 'static> {
    // block
    idempotent: u32,
    send_seq: u32,
    recv_seq: u32,
    reader: Option<&'a OwnedReadHalf>,
    // rbuf: [u8; g::DEFAULT_BUF_SIZE],
    user_data: Option<U>,
    builder: packet::Builder,
    // contorller
    shutdown_tx: broadcast::Sender<u8>,
    tx: async_channel::Sender<server::Packet>,
    rx: async_channel::Receiver<server::Packet>
}

impl<'a, U: Default + Send + Sync + 'static> Client<'a, U> {
    pub fn new(shutdown_tx: broadcast::Sender<u8>) -> Self {
        let (tx, rx) = async_channel::bounded(1);

        Self { 
            idempotent: 0, 
            send_seq: 0, 
            recv_seq: 0, 
            reader: None, 
            // rbuf: [0; g::DEFAULT_BUF_SIZE], 
            user_data: None, 
            builder: packet::Builder::new(), 
            shutdown_tx, 
            tx, 
            rx,
        }
    }

    pub fn new_arc(shutdown_tx: broadcast::Sender<u8>) -> Arc<Self> {
        Arc::new(Self::new(shutdown_tx))
    }

    pub(crate) fn load(&self, reader: &'a OwnedReadHalf) -> (
        tokio::sync::broadcast::Sender<u8>, 
        async_channel::Receiver<server::Packet>
    ) {
        unsafe {
            let this = &mut *(self as *const Self as *mut Self);
            this.reader = Some(reader);
        }
        (self.shutdown_tx.clone(), self.rx.clone())
    }

    pub fn reset(&mut self) {
        self.idempotent = 0;
        self.send_seq = 0;
        self.recv_seq = 0;
        self.reader = None;
        self.user_data = None;
        self.builder.reset();
    }

    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    pub fn send_seq(&self) -> u32 {
        self.send_seq
    }

    pub fn recv_seq(&self) -> u32 {
        self.recv_seq
    }

    pub fn sockfd(&self) -> Option<Socket> {
        #[cfg(windows)]
        { self.reader.as_ref().map(|r|r.as_ref().as_raw_socket()) }
        #[cfg(unix)]
        { self.reader.map(|r| r.as_ref().as_raw_fd()) }
    }

    pub fn local(&self) -> Option<SocketAddr> {
        self.reader.as_ref().map(|r|r.local_addr().expect("local_addr called failed"))
    }

    pub fn remote(&self) -> Option<SocketAddr> {
        self.reader.as_ref().map(|r|r.peer_addr().expect("peer_addr called failed"))
    }

    pub fn builder(&self) -> &mut packet::Builder {
        unsafe { &mut *(&self.builder as *const packet::Builder as *mut packet::Builder) }
    }

    pub fn shutdown(&self) {
        if let Err(err) = self.shutdown_tx.send(1) {
            tracing::error!("shutdown_tx.send failed: {err}");
        }
    }

    pub fn user_data(&self) -> Option<&U> {
        self.user_data.as_ref()
    }

    pub fn send(&self, pkt: server::Packet) -> g::Result<()> {
        if let Err(err) = self.tx.try_send(pkt) {
            return Err(g::Err::IOWriteFailed(format!("{err}")));
        }

        Ok(())
    }

    pub fn receiver(&self) -> async_channel::Receiver<server::Packet> {
        self.rx.clone()
    }
}
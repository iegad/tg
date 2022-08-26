pub mod conn;
pub mod pack;
pub mod tcp;
pub mod ws;

use crate::g;
use async_trait::async_trait;
use bytes::BytesMut;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::sync::Semaphore;

use self::conn::Conn;

// ----------------------------------- 工具函数 -----------------------------------
//
//
/// # sockaddr_to_bytes
///
/// 将 [std::net::SocketAddr] 转换成 IP字节和端口
///
/// IPV4: Vec<u8> 为  4 字节
/// IPV6: Vec<u8> 为 16 字节
///
/// # Example
///
/// ```
/// let (v, port) = tg::nw::sockaddr_to_bytes("0.0.0.0:6688".parse().unwrap()).unwrap();
/// assert_eq!(v.len(), 4);
/// assert_eq!(port, 6688);
/// ```
pub fn sockaddr_to_bytes(sock_addr: SocketAddr) -> g::Result<(Vec<u8>, u16)> {
    let ip = match sock_addr.ip() {
        IpAddr::V4(ip) => ip.octets().to_vec(),
        IpAddr::V6(ip) => ip.octets().to_vec(),
    };

    Ok((ip, sock_addr.port()))
}

/// bytes_to_sockaddr
///
/// 将 [(Vec<u8>, u16)] 转换成 [std::net::SocketAddr]
///
/// buf.len == [4 | 16];
///
/// ```
/// let ipv = vec![0u8; 4];
/// let sockaddr = tg::nw::bytes_to_sockaddr(&ipv, 6688).unwrap();
/// assert_eq!("0.0.0.0:6688", format!("{:?}", sockaddr));
/// ```
pub fn bytes_to_sockaddr(buf: &[u8], port: u16) -> g::Result<SocketAddr> {
    if port == 0 {
        return Err(g::Err::NwInvalidPort);
    }

    let addr: IpAddr = match buf.len() {
        4 => {
            let tmp: [u8; 4] = match buf[..4].try_into() {
                Ok(v) => v,
                Err(err) => return Err(g::Err::Custom(format!("{}", err))),
            };

            match tmp.try_into() {
                Ok(v) => v,
                Err(err) => return Err(g::Err::Custom(format!("{}", err))),
            }
        }

        16 => {
            let tmp: [u8; 16] = match buf[..16].try_into() {
                Ok(v) => v,
                Err(err) => return Err(g::Err::Custom(format!("{}", err))),
            };

            match tmp.try_into() {
                Ok(v) => v,
                Err(err) => return Err(g::Err::Custom(format!("{}", err))),
            }
        }

        _ => return Err(g::Err::NwInvalidIP),
    };

    Ok(SocketAddr::new(addr, port))
}

// ----------------------------------- 处理器接口 -----------------------------------
//
//
/// # IProc
///
/// 处理器接口
#[async_trait]
pub trait IProc: Copy + Clone + Send + Sync + 'static {
    /// # on_init
    ///
    /// 服务端初始化事件
    async fn on_init(&self, server: &Server) {
        println!("server[{:?}] is running", server.host())
    }

    /// # on_released
    ///
    /// 服务端释放事件
    async fn on_released(&self, server: &Server) {
        println!("server[{:?}] has released", server.host())
    }

    /// # on_connected
    ///
    /// 客户端连接事件
    async fn on_connected(&self, conn: &Conn) -> g::Result<()> {
        Ok(println!(
            "conn[{}:{:?}] has connected",
            conn.sockfd(),
            conn.remote()
        ))
    }

    /// # on_disconnected
    ///
    /// 客户端连接断开事件
    async fn on_disconnected(&self, conn: &Conn) {
        println!(
            "conn[{}:{:?}] has disconnected",
            conn.sockfd(),
            conn.remote()
        )
    }

    /// # on_conn_error
    ///
    /// 客户端错误事件
    async fn on_conn_error(&self, conn: &Conn, err: g::Err) {
        println!(
            "conn[{}:{:?}] error: {:?}",
            conn.sockfd(),
            conn.remote(),
            err
        );
    }

    /// # on_process
    ///
    /// 客户端消息处理事件
    async fn on_process(&self, conn: &Conn, req: &pack::Package) -> g::Result<BytesMut>;
}

// ----------------------------------- Server -----------------------------------
//
//
/// # Server
///
/// 服务对象
pub struct Server {
    max_connections: usize,
    limit_connections: Arc<Semaphore>,
    host: SocketAddr,
}

impl Server {
    /// # new
    ///
    /// 工厂方法
    ///
    /// @host: 监听地址, 例如, IPV4: 0.0.0.0:6688, IPV6: [::1]:6688
    ///
    /// @max_connections: 最大连接数
    ///
    /// # Error
    ///
    /// 当 host 不是正确的IPV4或IPV6监听地址时出现 g::Err::TcpSocketAddrInvalid错误
    pub fn new(host: &str, max_connections: usize) -> g::Result<Server> {
        let host: SocketAddr = match host.parse() {
            Ok(addr) => addr,
            Err(_) => return Err(g::Err::TcpSocketAddrInvalid(host.to_string())),
        };

        let max_connections = if max_connections == 0 {
            g::DEFAULT_MAX_CONNECTIONS
        } else {
            max_connections
        };

        Ok(Server {
            max_connections,
            limit_connections: Arc::new(Semaphore::new(max_connections)),
            host,
        })
    }

    /// # host
    ///
    /// 服务监听地址
    pub fn host(&self) -> &SocketAddr {
        &self.host
    }

    /// # max_connections
    ///
    /// 最大连接数
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// # current_connections
    ///
    /// 当前连接数
    pub fn current_connections(&self) -> usize {
        self.max_connections - self.limit_connections.available_permits()
    }
}

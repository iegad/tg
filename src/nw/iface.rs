use std::net::SocketAddr;

use crate::g;

use super::pack::Package;

pub trait IServer {
    fn host(&self) -> &SocketAddr;
    fn current_connections(&self) -> usize;
    fn max_connections(&self) -> usize;
}

pub trait IConn {
    fn remote(&self) -> &SocketAddr;
    fn local(&self) -> &SocketAddr;
    fn sockfd(&self) -> i32;
    fn send_seq(&self) -> u32;
    fn recv_seq(&self) -> u32;
}

pub trait IProc: Copy + Clone + Send + Sync + 'static {
    fn on_init(&self, server: &dyn IServer) -> g::Result<()> {
        Ok(println!("server[{:?}] is running", server.host()))
    }

    fn on_released(&self, server: &impl IServer) {
        println!("server[{:?}] has released", server.host())
    }

    fn on_connected(&self, conn: &dyn IConn) -> g::Result<()> {
        Ok(println!(
            "conn[{}:{:?}] has connected",
            conn.sockfd(),
            conn.remote()
        ))
    }

    fn on_disconnected(&self, conn: &impl IConn) {
        println!(
            "conn[{}:{:?}] has disconnected",
            conn.sockfd(),
            conn.remote()
        )
    }

    fn on_conn_error(&self, conn: &dyn IConn, err: g::Err) {
        println!(
            "conn[{}:{:?}] error: {:?}",
            conn.sockfd(),
            conn.remote(),
            err
        );
    }

    fn on_process(&self, conn: &dyn IConn, in_pack: &Package) -> g::Result<Vec<u8>>;
}

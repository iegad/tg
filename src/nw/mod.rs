//! nw 网络库, network 的缩写.
//! 
//! 目前只支持TCP协议, 后期目标支持 Websocket 和 KCP 协议.

pub mod conn;
pub mod server;
pub mod tcp;
pub mod tools;
pub mod packet;
pub mod client;

#[cfg(windows)]
pub type Socket = u64;
#[cfg(unix)]
pub type Socket = i32;
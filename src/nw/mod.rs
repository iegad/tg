pub mod conn;
pub mod server;
pub mod tcp;
pub mod tools;
pub mod packet;

#[cfg(windows)]
pub type Socket = u64;
#[cfg(unix)]
pub type Socket = i32;
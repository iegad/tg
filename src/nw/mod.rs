pub mod client;
pub mod conn;
pub mod pack;
pub mod server;
pub mod tcp;
pub mod tools;

#[cfg(windows)]
pub type Socket = u64;
#[cfg(unix)]
pub type Socket = i32;
pub mod client;
pub mod conn;
pub mod pack;
pub mod server;
pub mod tcp;
pub mod tools;

#[cfg(windows)]
pub type RawFd = u64;
#[cfg(unix)]
pub type RawFd = i32;
// ---------------------------
// tg::nw::pck
//     网络包定义
//
// @作者: iegad
//
// @时间: 2022-08-10
// ---------------------------

use crate::g;
use bytes::BytesMut;
use core::{fmt, slice::from_raw_parts};
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use std::sync::Arc;

lazy_static! {
    /// Package 对象池
    ///
    /// # Example
    ///
    /// ```
    /// let mut p1 = tg::nw::pack::PACK_POOL.pull();
    /// ```
    pub static ref PACK_POOL: Arc<PackagePool> = Arc::new(PackagePool::new(|| Package::new(), |v| {v.pos = 0;}));

    pub static ref REQ_POOL: Arc<PackagePool> = Arc::new(PackagePool::new(|| Package::new(), |v| {v.pos = 0;}));
    pub static ref RSP_POOL: Arc<PackagePool> = Arc::new(PackagePool::new(|| Package::new(), |v| {v.pos = 0;}));
}

/// 消息头
///
/// 消息头为定长 20 字节
///
/// # 内存布局
/// | [service_id] 2字节 | [package_id] 2字节 | [router_id] 4字节 | [idempotent] 4字节 | [token] 4字节 | [len] 4字节 |
///
/// ## service_id 服务ID
///
/// 每个服务都有一个 2字节ID
///
/// ## router_id 路由ID
///
/// 分布式中, 同一个service会有多个服务, router_id用于区分, 服务节点
///
/// ## package_id 消息ID
///
/// 用于区分消息包类型, 跟据此字段来处理消息请求
///
/// ## idempotent 幂等
///
/// 幂等, 用于检测包是否为重复包.
///
/// ## len
///
/// 消息长度, 消息原始长度 消息头[10 bytes] + 消息体[N bytes].
///
/// ## token
///
/// 用于检测客户端是否合法
pub struct Package {
    service_id: u16,
    package_id: u16,
    router_id: u32,
    idempotent: u32,
    token: u32,
    len: usize,
    data: Vec<u8>,

    pos: usize,
    wbuf: BytesMut,
}

type PackagePool = LinearObjectPool<Package>;

impl Package {
    /// Package 原始数据初始化长度.
    /// 随着后期的调用, 原始数据长度为发生变化, 但决不会小于 [Pack::RAW_SIZE].
    pub const DEFAULT_RAW_SIZE: usize = 4096;
    pub const HEAD_SIZE: usize = 20;
    pub const MAX_SIZE: usize = 1024 * 1024 * 1024;
    const HEAD_KEY_16: u16 = 0xFBFA;
    const HEAD_KEY_32: u32 = 0xFFFEFDFC;

    /// 创建一个空包对象
    pub fn new() -> Self {
        Self {
            service_id: 0,
            package_id: 0,
            router_id: 0,
            idempotent: 0,
            token: 0,
            len: 0,
            data: vec![0u8; g::DEFAULT_BUF_SIZE],

            pos: 0,
            wbuf: BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        }
    }

    /// 创建一个有初始化值的包对象
    pub fn with_params(
        service_id: u16,
        router_id: u32,
        package_id: u16,
        idempotent: u32,
        token: u32,
        data: &[u8],
    ) -> Package {
        let len = data.len();
        Self {
            service_id,
            package_id,
            router_id,
            idempotent,
            token,
            len,
            data: data.to_vec(),

            pos: 0,
            wbuf: BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        }
    }

    /// 将 self.raw[..n] 转换为 package.
    ///
    /// 成功转换为一个完整的包返回 true.
    ///
    /// 未成功转换为一个完整的包(后续还需要追加码流才能成功完整的Package) 返回 false.
    ///
    /// 无效的码流, 返回相应错误.
    pub fn parse_buf(&mut self, buf: &mut BytesMut) -> g::Result<bool> {
        let ptr = buf.as_ptr();
        let mut pos = 0;
        let len = buf.len();
        if self.pos == 0 {
            pos = Self::HEAD_SIZE;

            unsafe {
                self.service_id = *(ptr as *const u16) ^ Self::HEAD_KEY_16;
                self.package_id = *(ptr.add(2) as *const u16) ^ Self::HEAD_KEY_16;
                self.router_id = *(ptr.add(4) as *const u32) ^ Self::HEAD_KEY_32;
                self.idempotent = *(ptr.add(8) as *const u32) ^ Self::HEAD_KEY_32;
                self.token = *(ptr.add(12) as *const u32) ^ Self::HEAD_KEY_32;
                self.len = (*(ptr.add(16) as *const u32) ^ Self::HEAD_KEY_32) as usize;
            }

            if self.len > Self::MAX_SIZE {
                return Err(g::Err::PackTooLong);
            }

            if self.data.capacity() < self.len {
                self.data.resize(self.len, 0);
            }
        }

        let body_len = len - pos;
        self.data[self.pos..self.pos + body_len].copy_from_slice(&buf[pos..len]);
        self.pos += body_len;

        if self.pos > self.len {
            return Err(g::Err::PackTooLong);
        }

        let res = self.pos == self.len;
        if res {
            self.pos = 0
        }

        buf.clear();
        Ok(res)
    }

    pub fn parse(&mut self, buf: &[u8]) -> g::Result<bool> {
        let ptr = buf.as_ptr();
        let mut pos = 0;
        let len = buf.len();
        if self.pos == 0 {
            pos = Self::HEAD_SIZE;

            unsafe {
                self.service_id = *(ptr as *const u16) ^ Self::HEAD_KEY_16;
                self.package_id = *(ptr.add(2) as *const u16) ^ Self::HEAD_KEY_16;
                self.router_id = *(ptr.add(4) as *const u32) ^ Self::HEAD_KEY_32;
                self.idempotent = *(ptr.add(8) as *const u32) ^ Self::HEAD_KEY_32;
                self.token = *(ptr.add(12) as *const u32) ^ Self::HEAD_KEY_32;
                self.len = (*(ptr.add(16) as *const u32) ^ Self::HEAD_KEY_32) as usize;
            }

            if self.len > Self::MAX_SIZE {
                return Err(g::Err::PackTooLong);
            }

            if self.data.capacity() < self.len {
                self.data.resize(self.len, 0);
            }
        }

        self.data[self.pos..self.pos + len - pos].copy_from_slice(&buf[pos..len]);
        self.pos += len - pos;

        if self.pos > self.len {
            return Err(g::Err::PackTooLong);
        }

        let res = self.pos == self.len;
        if res {
            self.pos = 0
        }

        Ok(res)
    }

    /// 返回源始码流
    pub fn to_bytes(&self) -> BytesMut {
        let mut wbuf = self.wbuf.clone();
        wbuf.clear();
        let len = self.len + Self::HEAD_SIZE;

        if wbuf.capacity() < len {
            wbuf.resize(len, 0);
        }

        unsafe {
            wbuf[..2].copy_from_slice(from_raw_parts(
                &(self.service_id ^ Self::HEAD_KEY_16) as *const u16 as *const u8,
                2,
            ));

            wbuf[2..4].copy_from_slice(from_raw_parts(
                &(self.package_id ^ Self::HEAD_KEY_16) as *const u16 as *const u8,
                2,
            ));

            wbuf[4..8].copy_from_slice(from_raw_parts(
                &(self.router_id ^ Self::HEAD_KEY_32) as *const u32 as *const u8,
                4,
            ));

            wbuf[8..12].copy_from_slice(from_raw_parts(
                &(self.idempotent ^ Self::HEAD_KEY_32) as *const u32 as *const u8,
                4,
            ));

            wbuf[12..16].copy_from_slice(from_raw_parts(
                &(self.token ^ Self::HEAD_KEY_32) as *const u32 as *const u8,
                4,
            ));

            wbuf[16..20].copy_from_slice(from_raw_parts(
                &((self.len as u32) ^ Self::HEAD_KEY_32) as *const u32 as *const u8,
                4,
            ));
        }

        wbuf[Self::HEAD_SIZE..len].copy_from_slice(&self.data());
        wbuf
    }

    /// 返回 service_id
    pub fn service_id(&self) -> u16 {
        self.service_id
    }

    /// 设置 pid
    pub fn set_service_id(&mut self, service_id: u16) {
        self.service_id = service_id
    }

    pub fn router_id(&self) -> u32 {
        self.router_id
    }

    pub fn set_router_id(&mut self, router_id: u32) {
        self.router_id = router_id;
    }

    pub fn package_id(&self) -> u16 {
        self.package_id
    }

    pub fn set_package_id(&mut self, package_id: u16) {
        self.package_id = package_id;
    }

    /// 返回 幂等
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    /// 设置 幂等
    pub fn set_idempotent(&mut self, idempotent: u32) {
        self.idempotent = idempotent;
    }

    /// 获取消息体数据
    pub fn data(&self) -> &[u8] {
        &self.data[..self.len]
    }

    /// 设置消息体数据
    pub fn set_data(&mut self, data: &[u8]) {
        let len = data.len();
        if self.data.capacity() < len {
            self.data.resize(len, 0);
        }

        self.data[..len].copy_from_slice(data);
        self.len = len;
    }

    pub fn token(&self) -> u32 {
        self.token
    }

    pub fn set_token(&mut self, token: u32) {
        self.token = token;
    }
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SID[{}], PID[{}], RID[{}], IDE[{}], TOK[{}], LEN[{}], DATA{:?}",
            self.service_id,
            self.package_id,
            self.router_id,
            self.idempotent,
            self.token,
            self.len,
            self.data(),
        )
    }
}

#[cfg(test)]
mod pcomp_tester {
    use super::Package;
    use crate::{nw::pack::PACK_POOL, utils};

    #[test]
    fn test_package() {
        let s = "hello world";
        let p1 = Package::with_params(0x01, 0x02, 0x03, 0x04, 0x05, s.as_bytes());
        assert_eq!(p1.service_id(), 0x01);
        assert_eq!(p1.router_id(), 0x02);
        assert_eq!(p1.package_id(), 0x03);
        assert_eq!(p1.idempotent(), 0x04);
        assert_eq!(p1.token(), 0x05);
        assert_eq!(p1.data().len(), s.len());
        assert_eq!(s, core::str::from_utf8(p1.data()).unwrap());

        let mut wbuf = p1.to_bytes();
        println!("{}", utils::bytes_to_hex(&wbuf));

        let mut p2 = PACK_POOL.pull();
        assert!(p2.parse_buf(&mut wbuf).unwrap());

        assert_eq!(p1.service_id(), p2.service_id());
        assert_eq!(p1.router_id(), p2.router_id());
        assert_eq!(p1.package_id(), p2.package_id());
        assert_eq!(p1.idempotent(), p2.idempotent());
        assert_eq!(p1.token(), p2.token());
        assert_eq!(p1.data().len(), p2.data().len());
        assert_eq!(
            core::str::from_utf8(p1.data()).unwrap(),
            core::str::from_utf8(p2.data()).unwrap()
        );
    }
}

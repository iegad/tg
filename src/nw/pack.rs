/// -----------------------------------
/// Package 消息包
/// author: iegad
/// time:   2022-09-20
/// update_timeline:
/// | ---- time ---- | ---- editor ---- | ------------------- content -------------------

use crate::g;
use bytes::{Buf, BufMut, BytesMut};
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use std::mem::size_of;
use type_layout::TypeLayout;

/// # Package 消息包
/// 
/// 用于分布式网络通信
/// 
/// 消息包由消息头与消息体组成
/// 
/// ## 消息头包含以下五个字段:
/// 
/// [service_id]: 16位: 服务ID, 确定所调用的服务
/// 
/// [package_id]: 16位: 消息ID, 确定所调用服务的业务句柄
/// 
/// [router_id]:  32位: 路由ID, 确定所调用服务的节点
/// 
/// [idempotent]: 32位: 幂等
/// 
/// [raw_len]:    32位: 消息长度, 包括消息头16字节与消息体长度
#[derive(TypeLayout, Debug)]
#[repr(C)]
pub struct Package {
    service_id: u16, // 服务ID
    package_id: u16, // 包ID
    router_id: u32,  // 路由ID
    idempotent: u32, // 幂等
    raw_len: usize,  // 包长
    data: BytesMut,  // 消息体
}

lazy_static! {
    /// PACK_POOL: 框架内部使用 package 对象池
    pub(crate) static ref PACK_POOL: LinearObjectPool<Package> =
        LinearObjectPool::new(|| Package::new(), |v| { v.reset() });

    /// REQ_POOL: 请求使用 package 对象池
    pub static ref REQ_POOL: LinearObjectPool<Package> =
        LinearObjectPool::new(|| Package::new(), |v| { v.reset() });

    /// RSP_POOL: 应答使用 package 对象池
    pub static ref RSP_POOL: LinearObjectPool<Package> =
        LinearObjectPool::new(|| Package::new(), |v| { v.reset() });
}

impl Package {
    /// 消息头长度
    pub const HEAD_SIZE: usize = size_of::<u16>()
        + size_of::<u16>()
        + size_of::<u32>()
        + size_of::<u32>()
        + size_of::<u32>();
    
    /// 消息体最大长度: 1G
    pub const MAX_DATA_SIZE: usize = 1024 * 1024 * 1024;

    /// 16位加密KEY, 用于消息头中16位字段的加密和解密
    const HEAD_16_KEY: u16 = 0x0A0B;

    /// 32位加密KEY, 用于消息头中32位字段的加密和解密
    const HEAD_32_KEY: u32 = 0x0C0D0E0F;

    /// 创建空包Package, 空包长度为 Package::HEAD_SIZE
    pub fn new() -> Self {
        Self {
            service_id: 0,
            package_id: 0,
            router_id: 0,
            idempotent: 0,
            raw_len: Self::HEAD_SIZE,
            data: BytesMut::new(),
        }
    }

    /// 通过入参创建 Package
    pub fn with_params(
        service_id: u16,
        package_id: u16,
        router_id: u32,
        idempotent: u32,
        data: &[u8],
    ) -> Self {
        Self {
            service_id,
            package_id,
            router_id,
            idempotent,
            raw_len: data.len() + Self::HEAD_SIZE,
            data: BytesMut::from(data),
        }
    }

    /// 通过 rbuf 缓冲区构建 pack
    #[inline(always)]
    pub fn parse(rbuf: &mut BytesMut, pack: &mut Self) -> g::Result<()> {
        if pack.valid() {
            return Ok(())
        }

        if pack.head_valid() {
            if rbuf.len() == 0 {
                return Err(g::Err::PackDataSizeInvalid);
            }

            pack.fill_data(rbuf);
            return Ok(())
        }

        if rbuf.len() < Self::HEAD_SIZE {
            return Err(g::Err::PackDataSizeInvalid);
        }

        pack.from_buf(rbuf)?;
        Ok(())
    }

    /// 反序列化, rbuf 缓冲区中必需有一个完整的包
    pub fn from_bytes(rbuf: &mut BytesMut) -> g::Result<Self> {
        if rbuf.len() < Self::HEAD_SIZE {
            return Err(g::Err::PackHeadInvalid("Head Size is invalid"));
        }

        let service_id = rbuf.get_u16_le() ^ Self::HEAD_16_KEY;
        if service_id == 0 {
            return Err(g::Err::PackHeadInvalid("Package.service_id is invalid"));
        }

        let package_id = rbuf.get_u16_le() ^ Self::HEAD_16_KEY;
        if package_id == 0 {
            return Err(g::Err::PackHeadInvalid("Package.package_id is invalid"));
        }

        let router_id = rbuf.get_u32_le() ^ Self::HEAD_32_KEY;
        if router_id == 0 {
            return Err(g::Err::PackHeadInvalid("Package.router_id is invalid"));
        }

        let idempotent = rbuf.get_u32_le() ^ Self::HEAD_32_KEY;
        if idempotent == 0 {
            return Err(g::Err::PackHeadInvalid("Package.idempotent is invalid"));
        }

        let raw_len = (rbuf.get_u32_le() ^ Self::HEAD_32_KEY) as usize;
        if raw_len < Self::HEAD_SIZE {
            return Err(g::Err::PackHeadInvalid("Package is illegal"));
        }

        if raw_len > Self::HEAD_SIZE + Self::MAX_DATA_SIZE {
            return Err(g::Err::PackDataSizeInvalid);
        }

        let data_len = raw_len - Self::HEAD_SIZE;

        if data_len > rbuf.len() {
            return Err(g::Err::PackDataSizeInvalid);
        }

        Ok(Self {
            service_id,
            package_id,
            router_id,
            idempotent,
            raw_len,
            data: rbuf.split_to(data_len),
        })
    }

    /// 通过BytesMut 缓冲区构建当前 Package对象
    fn from_buf(&mut self, rbuf: &mut BytesMut) -> g::Result<()> {
        if rbuf.len() < Self::HEAD_SIZE {
            return Err(g::Err::PackHeadInvalid("Head Size is invalid"));
        }

        self.service_id = rbuf.get_u16_le() ^ Self::HEAD_16_KEY;
        if self.service_id == 0 {
            return Err(g::Err::PackHeadInvalid("Package.service_id is invalid"));
        }

        self.package_id = rbuf.get_u16_le() ^ Self::HEAD_16_KEY;
        if self.package_id == 0 {
            return Err(g::Err::PackHeadInvalid("Package.package_id is invalid"));
        }

        self.router_id = rbuf.get_u32_le() ^ Self::HEAD_32_KEY;
        if self.router_id == 0 {
            return Err(g::Err::PackHeadInvalid("Package.router_id is invalid"));
        }

        self.idempotent = rbuf.get_u32_le() ^ Self::HEAD_32_KEY;
        if self.idempotent == 0 {
            return Err(g::Err::PackHeadInvalid("Package.idempotent is invalid"));
        }

        self.raw_len = (rbuf.get_u32_le() ^ Self::HEAD_32_KEY) as usize;
        if self.raw_len < Self::HEAD_SIZE {
            return Err(g::Err::PackHeadInvalid("Package is illegal"));
        }

        if self.raw_len > Self::HEAD_SIZE + Self::MAX_DATA_SIZE {
            return Err(g::Err::PackDataSizeInvalid);
        }

        if self.raw_len > Self::HEAD_SIZE {
            let rbuf_len = rbuf.len();
            let mut data_len = self.raw_len - Self::HEAD_SIZE;
            if rbuf_len < data_len {
                data_len = rbuf_len;
            }

            self.data = rbuf.split_to(data_len);
        }
        
        Ok(())
    }

    /// 按需填充消息体, 当该包被加载不完全时调用.
    /// 
    /// 当包的消息头已构建, 但是消息体不全时调用此方法.
    /// 
    /// 如果 rbuf 缓冲区的内容大于 当前Package 消息体所需长度时, 消息体只会从rbuf 中消费掉需要的数据长度.
    /// 
    /// 该函数不同于[append_data], 该函数是在接收包时构建Package中使用.
    #[inline(always)]
    fn fill_data(&mut self, rbuf: &mut BytesMut) {
        debug_assert!(self.head_valid());

        let rbuf_len = rbuf.len();

        let mut left_len = self.raw_len - Self::HEAD_SIZE - self.data.len();
        if left_len > rbuf_len {
            left_len = rbuf_len;
        }

        self.data.extend_from_slice(&rbuf[0..left_len]);
        rbuf.advance(left_len);
    }

    /// 重置 Package
    #[inline(always)]
    pub fn reset(&mut self) {
        self.service_id = 0;
    }

    /// 将Package包 序列化到一个新的 BytesMut字节缓冲区
    #[inline(always)]
    pub fn to_bytes(&self) -> BytesMut {
        let mut wbuf = BytesMut::with_capacity(self.raw_len);

        wbuf.put_u16_le(self.service_id ^ Self::HEAD_16_KEY);
        wbuf.put_u16_le(self.package_id ^ Self::HEAD_16_KEY);
        wbuf.put_u32_le(self.router_id ^ Self::HEAD_32_KEY);
        wbuf.put_u32_le(self.idempotent ^ Self::HEAD_32_KEY);
        wbuf.put_u32_le((self.raw_len as u32) ^ Self::HEAD_32_KEY);
        if self.raw_len > Self::HEAD_SIZE {
            wbuf.put(&self.data[..]);
        }

        wbuf
    }

    /// 追加数据到消息体中
    /// 
    /// 该函数不同于[fill_data], 该函数是在主动发包时构建Package中使用.
    #[inline(always)]
    pub fn append_data(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
        self.raw_len += data.len();

        debug_assert!(self.data.len() <= Self::MAX_DATA_SIZE);
    }

    /// 消息头是否有效
    #[inline(always)]
    pub fn head_valid(&self) -> bool {
        self.service_id > 0 && self.package_id > 0 && self.router_id > 0 && self.idempotent > 0
    }

    /// 包是否有效
    #[inline(always)]
    pub fn valid(&self) -> bool {
        self.raw_len == Self::HEAD_SIZE + self.data.len() && self.head_valid()
    }

    #[inline(always)]
    pub fn set_service_id(&mut self, service_id: u16) {
        self.service_id = service_id;
    }

    #[inline(always)]
    pub fn service_id(&self) -> u16 {
        self.service_id
    }

    #[inline(always)]
    pub fn set_package_id(&mut self, package_id: u16) {
        self.package_id = package_id;
    }

    #[inline(always)]
    pub fn package_id(&self) -> u16 {
        self.package_id
    }

    #[inline(always)]
    pub fn set_router_id(&mut self, router_id: u32) {
        self.router_id = router_id;
    }

    #[inline(always)]
    pub fn router_id(&self) -> u32 {
        self.router_id
    }

    #[inline(always)]
    pub fn set_idempotent(&mut self, idempotent: u32) {
        self.idempotent = idempotent;
    }

    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    #[inline(always)]
    pub fn set_data(&mut self, data: &[u8]) {
        assert!(data.len() <= Self::MAX_DATA_SIZE);

        self.data.clear();
        self.data.extend_from_slice(data);

        self.raw_len = Self::HEAD_SIZE + data.len();
    }

    #[inline(always)]
    pub fn data(&self) -> &[u8] {
        &self.data[..self.raw_len - Self::HEAD_SIZE]
    }

    #[inline(always)]
    pub fn raw_len(&self) -> usize {
        self.raw_len
    }
}

#[cfg(test)]
mod pack_test {
    use super::Package;
    use bytes::{BufMut, BytesMut};
    use type_layout::TypeLayout;

    #[test]
    fn test_package_info() {
        println!(
       "* --------- PACKAGE INFO BEGIN ---------\n\
        * Layout: {}\n\
        * Demo: {:#?}\n\
        * --------- PACKAGE INFO END ---------\n", 
        Package::type_layout(), 
        Package::with_params(1, 2, 3, 4, b"Hello world"));
    }

    #[test]
    fn test_package() {
        let data = "Hello world";

        let mut p1 = Package::new();
        assert!(!p1.valid());

        p1.set_service_id(1);
        p1.set_package_id(2);
        p1.set_router_id(3);
        p1.set_idempotent(4);
        p1.set_data(data.as_bytes());

        assert!(p1.valid());
        assert_eq!(p1.service_id(), 1);
        assert_eq!(p1.package_id(), 2);
        assert_eq!(p1.router_id(), 3);
        assert_eq!(p1.data().len(), data.len());
        assert_eq!(p1.raw_len(), data.len() + Package::HEAD_SIZE);
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), data);

        let p2 = Package::with_params(1, 2, 3, 4, data.as_bytes());
        assert!(p2.valid());
        assert_eq!(p2.service_id(), 1);
        assert_eq!(p2.package_id(), 2);
        assert_eq!(p2.router_id(), 3);
        assert_eq!(p2.data().len(), data.len());
        assert_eq!(p2.raw_len(), data.len() + Package::HEAD_SIZE);
        assert_eq!(std::str::from_utf8(p2.data()).unwrap(), data);

        let mut iobuf = BytesMut::with_capacity(1500);
        let buf = p1.to_bytes();
        iobuf.put(&buf[..]);
        let iobuf_pos = &iobuf[0] as *const u8;
        assert_eq!(iobuf.capacity(), 1500);
        assert_eq!(iobuf.len(), buf.len());
        assert_eq!(buf.len(), data.len() + Package::HEAD_SIZE);

        let p3 = Package::from_bytes(&mut iobuf).unwrap();
        assert_eq!(iobuf.capacity(), 1500 - buf.len());

        iobuf.put_bytes(0, 1);
        let iobuf_pos_ = &iobuf[0] as *const u8;
        assert_eq!(iobuf_pos_ as usize - iobuf_pos as usize, buf.len());

        assert!(p3.valid());
        assert_eq!(p3.service_id(), 1);
        assert_eq!(p3.package_id(), 2);
        assert_eq!(p3.router_id(), 3);
        assert_eq!(p3.data().len(), 11);
        assert_eq!(p3.raw_len(), 27);
        assert_eq!(std::str::from_utf8(p3.data()).unwrap(), data);

        let mut p4 = Package::new();
        let mut buf = p3.to_bytes();
        p4.from_buf(&mut buf).unwrap();

        assert!(p4.valid());
        assert_eq!(p4.service_id(), 1);
        assert_eq!(p4.package_id(), 2);
        assert_eq!(p4.router_id(), 3);
        assert_eq!(p4.data().len(), 11);
        assert_eq!(p4.raw_len(), 27);
        assert_eq!(std::str::from_utf8(p4.data()).unwrap(), data);
    }
}

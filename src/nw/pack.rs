use crate::g;
use bytes::{Buf, BufMut, BytesMut};
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use std::mem::size_of;
use type_layout::TypeLayout;

lazy_static! {
    /// # PACK_POOL 
    /// 
    /// for internal to use
    pub(crate) static ref PACK_POOL: LinearObjectPool<Package> =
        LinearObjectPool::new(|| Package::new(), |v| { v.reset() });

    /// # REQ_POOL
    /// 
    /// you can pull one from package's pool when need a package for request.
    pub static ref REQ_POOL: LinearObjectPool<Package> =
        LinearObjectPool::new(|| Package::new(), |v| { v.reset() });

    /// # WBUF_POOL
    /// 
    /// you can pull one from BytesMut's pool when need a BytesMut for write buffer.
    pub static ref WBUF_POOL: LinearObjectPool<BytesMut> =
        LinearObjectPool::new(|| BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        |v| {
            if v.capacity() < g::DEFAULT_BUF_SIZE {
                v.resize(g::DEFAULT_BUF_SIZE, 0);
            }
            v.clear();
        });
}

/// # Package 
///
/// for network transport
///
/// [Package] composit by Head and Body
///
/// # Head:
/// 
/// Head's size is 16 Bytes.
///
/// [service_id]: 16bit: match service.
///
/// [package_id]: 16bit: match service's implement handler.
///
/// [router_id]:  32bit: match service's node when service has multiple nodes.
///
/// [idempotent]: 32bit: check if the package has already been processed.
///
/// [raw_len]:    32bit: Package's length, include Head's size and Body's size.
/// 
/// # Memory Layout
/// 
/// | service_id: 2 Bytes | package_id: 2 Bytes | router_id: 4 Bytes | idempotent: 4 Bytes | raw_len: 4 Bytes | data: [[0, 1G Bytes]]
/// 
/// # Example
/// 
/// ```
/// let p1 = Package::new();
/// assert_eq!(Package::HEAD_SIZE, p1.raw_len());
/// 
/// let data = b"Hello world";
/// let p2 = Package::with_params(1, 2, 3, 4, data.len(), data);
/// assert_eq!(p2.service_id(), 1);
/// assert_eq!(p2.package_id(), 2);
/// assert_eq!(p2.router_id(), 3);
/// assert_eq!(p2.idempotent(), 4);
/// assert_eq!(p2.raw_len(), Package::HEAD_SIZE + data.len());
/// ```
/// 
/// # updates history
#[derive(TypeLayout, Debug)]
#[repr(C)]
pub struct Package {
    service_id: u16, 
    package_id: u16, 
    router_id: u32,  
    idempotent: u32, 
    raw_len: usize,  
    data: BytesMut,  
}

impl Package {
    /// Package head's size
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

    /// # new
    /// 
    /// build a default Package. Package.raw_len is Package::HEAD_SIZE.
    /// 
    /// # Example
    /// 
    /// ```
    /// use tg::nw::pack::Package;
    /// 
    /// let p = Package::new();
    /// assert_eq!(p.raw_len(), Package::HEAD_SIZE);
    /// ```
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

    /// # with_params:
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
    #[inline]
    pub fn parse(rbuf: &mut BytesMut, pack: &mut Self) -> g::Result<()> {
        if pack.valid() {
            return Ok(());
        }

        if pack.head_valid() {
            if rbuf.len() == 0 {
                return Err(g::Err::PackDataSizeInvalid);
            }

            pack.fill_data(rbuf);
            return Ok(());
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
    #[inline]
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
    #[inline]
    pub fn reset(&mut self) {
        self.service_id = 0;
    }

    /// 将Package包 序列化到一个新的 BytesMut字节缓冲区
    #[inline]
    pub fn to_bytes(&self, wbuf: &mut BytesMut) {
        wbuf.put_u16_le(self.service_id ^ Self::HEAD_16_KEY);
        wbuf.put_u16_le(self.package_id ^ Self::HEAD_16_KEY);
        wbuf.put_u32_le(self.router_id ^ Self::HEAD_32_KEY);
        wbuf.put_u32_le(self.idempotent ^ Self::HEAD_32_KEY);
        wbuf.put_u32_le((self.raw_len as u32) ^ Self::HEAD_32_KEY);
        if self.raw_len > Self::HEAD_SIZE {
            wbuf.put(&self.data[..]);
        }
    }

    /// 追加数据到消息体中
    ///
    /// 该函数不同于[fill_data], 该函数是在主动发包时构建Package中使用.
    #[inline]
    pub fn append_data(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
        self.raw_len += data.len();

        debug_assert!(self.data.len() <= Self::MAX_DATA_SIZE);
    }

    /// 消息头是否有效
    #[inline]
    pub fn head_valid(&self) -> bool {
        self.service_id > 0 && self.package_id > 0 && self.router_id > 0 && self.idempotent > 0
    }

    /// 包是否有效
    #[inline]
    pub fn valid(&self) -> bool {
        self.raw_len == Self::HEAD_SIZE + self.data.len() && self.head_valid()
    }

    #[inline]
    pub fn set_service_id(&mut self, service_id: u16) {
        self.service_id = service_id;
    }

    #[inline]
    pub fn service_id(&self) -> u16 {
        self.service_id
    }

    #[inline]
    pub fn set_package_id(&mut self, package_id: u16) {
        self.package_id = package_id;
    }

    #[inline]
    pub fn package_id(&self) -> u16 {
        self.package_id
    }

    #[inline]
    pub fn set_router_id(&mut self, router_id: u32) {
        self.router_id = router_id;
    }

    #[inline]
    pub fn router_id(&self) -> u32 {
        self.router_id
    }

    #[inline]
    pub fn set_idempotent(&mut self, idempotent: u32) {
        self.idempotent = idempotent;
    }

    #[inline]
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    #[inline]
    pub fn set_data(&mut self, data: &[u8]) {
        assert!(data.len() <= Self::MAX_DATA_SIZE);

        self.data.clear();
        self.data.extend_from_slice(data);

        self.raw_len = Self::HEAD_SIZE + data.len();
    }

    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data[..self.raw_len - Self::HEAD_SIZE]
    }

    #[inline]
    pub fn raw_len(&self) -> usize {
        self.raw_len
    }
}

#[cfg(test)]
mod pack_test {
    use crate::nw::pack::WBUF_POOL;

    use super::Package;
    use bytes::{BufMut, BytesMut};
    use type_layout::TypeLayout;

    #[test]
    fn test_package_info() {
        assert_eq!(Package::HEAD_SIZE, 16);

        println!(
            "* --------- PACKAGE INFO BEGIN ---------\n\
        * Layout: {}\n\
        * Demo: {:#?}\n\
        * --------- PACKAGE INFO END ---------\n",
            Package::type_layout(),
            Package::with_params(1, 2, 3, 4, b"Hello world")
        );
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
        let mut buf = WBUF_POOL.pull();
        p1.to_bytes(&mut buf);
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
        p3.to_bytes(&mut buf);
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

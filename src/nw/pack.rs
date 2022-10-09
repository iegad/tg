use bytes::{BytesMut, BufMut};
use crate::g;
use lazy_static::lazy_static;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use std::{sync::Arc, fmt::Display};
use type_layout::TypeLayout;

lazy_static! {
    pub static ref WBUF_POOL: LinearObjectPool<BytesMut> =
        LinearObjectPool::new(|| BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        |v| {
            if v.capacity() < g::DEFAULT_BUF_SIZE {
                v.resize(g::DEFAULT_BUF_SIZE, 0);
            }
            unsafe{ v.set_len(0); }
        });

    pub(crate) static ref PACK_POOL: LinearObjectPool<Package> = LinearObjectPool::new(Package::new, |v|v.reset());
    pub static ref REQ_POOL: LinearObjectPool<Package> = LinearObjectPool::new(Package::new, |v|v.reset());
}

pub type PackBuf = Arc<LinearReusable<'static, BytesMut>>;

#[derive(TypeLayout, Debug)]
pub struct Package {
    raw_pos: usize,
    raw: Vec::<u8>,
}

unsafe impl Sync for Package{}
unsafe impl Send for Package{}

impl Package {
    pub const HEAD_SIZE: usize = 12;

    pub fn new() -> Self {
        Self { 
            raw: vec![0; g::DEFAULT_BUF_SIZE],
            raw_pos: 0,
        }
    }

    pub fn with_params(package_id: u16, idempotent: u32, data: &[u8]) -> Self {
        let mut res = Self::new();
        res.set_package_id(package_id);
        res.set_idempotent(idempotent);
        res.set_data(data);
        res.active();
        res
    }

    #[inline]
    pub fn reset(&mut self) {
        self.raw_pos = 0;
    }

    #[inline]
    pub fn to_bytes(&self, buf: &mut BytesMut) -> g::Result<()> {
        assert!(self.valid());
        buf.put(&self.raw[..self.raw_len()]);
        Ok(())
    }

    pub fn from_bytes(&mut self, buf: &[u8]) -> g::Result<usize> {
        let buf_len = buf.len();
        let mut buf_pos = 0;

        assert!(buf_len > 0);

        // 处理消息头
        if self.raw_pos < Self::HEAD_SIZE {
            let mut nleft = Self::HEAD_SIZE - self.raw_pos;
            if buf_len < nleft {
                nleft = buf_len;
            }

            self.raw[self.raw_pos..nleft].copy_from_slice(&buf[..nleft]);
            self.raw_pos += nleft;
            buf_pos += nleft;

            if self.raw_pos == Self::HEAD_SIZE &&  self.raw[0] ^ self.raw[9] != self.head_code() {
                return Err(g::Err::PackHeadInvalid);
            }
        }

        // 处理消息体
        if self.raw_pos >= Self::HEAD_SIZE {
            let raw_len = self.raw_len();
            if buf_len > buf_pos /* buf 中还有多余的数据未读取 */ && self.raw_pos < raw_len /* 消息体没有读满 */ {
                if self.raw.capacity() < raw_len {
                    self.raw.resize(raw_len, 0);
                }

                let mut nleft = raw_len - self.raw_pos;
                let n = buf_len - buf_pos;
                if nleft > n {
                    nleft = n;
                }
    
                self.raw[self.raw_pos..self.raw_pos + nleft].copy_from_slice(&buf[buf_pos.. buf_pos + nleft]);
                self.raw_pos += nleft;
                buf_pos += nleft;
            }
        }

        Ok(buf_pos)
    }

    #[inline]
    pub fn active(&mut self) {
        assert!(self.package_id() > 0 && self.idempotent() > 0 && self.raw_len() <= self.raw.len());

        unsafe {
            let p = self.raw.as_ptr();

            let head_code = p.add(10) as *mut u8;
            *head_code = self.raw[0] ^ self.raw[9];

            let raw_code = p.add(11) as *mut u8;
            *raw_code = self.raw[0] ^ self.raw[self.raw_len()-1];
        }

        self.raw_pos = self.raw_len()
    }

    #[inline]
    pub fn valid(&self) -> bool {
        self.package_id() > 0 && self.idempotent() > 0 && self.raw_pos == self.raw_len()
    }

    #[inline]
    pub fn set_package_id(&mut self, package_id: u16) {
        assert!(package_id > 0);
        self.raw[0..2].copy_from_slice(&package_id.to_le_bytes());
    }

    #[inline]
    pub fn package_id(&self) -> u16 {
        unsafe {*(self.raw.as_ptr() as *const u8 as *const u16)}
    }

    #[inline]
    pub fn set_idempotent(&mut self, idempotent: u32) {
        assert!(idempotent > 0);
        self.raw[2..6].copy_from_slice(&idempotent.to_le_bytes());
    }

    #[inline]
    pub fn idempotent(&self) -> u32 {
        unsafe {*(self.raw.as_ptr().add(2) as *const u8 as *const u32)}
    }

    #[inline]
    pub fn head_code(&self) -> u8 {
        unsafe {*(self.raw.as_ptr().add(10) as *const u8)}
    }

    #[inline]
    pub fn data_code(&self) -> u8 {
        unsafe {*(self.raw.as_ptr().add(11) as *const u8)}
    }

    #[inline]
    pub fn raw_len(&self) -> usize {
        unsafe {*(self.raw.as_ptr().add(6) as *const u8 as *const u32) as usize}
    }

    #[inline]
    pub fn set_data(&mut self, data: &[u8]) {
        let data_len = data.len();
        let raw_len = data_len + Self::HEAD_SIZE;

        if self.raw.capacity() < raw_len {
            self.raw.resize(raw_len, 0);
        }

        let rawlen = raw_len as u32;
        self.raw[6..10].copy_from_slice(&rawlen.to_le_bytes());
        self.raw[Self::HEAD_SIZE..raw_len].copy_from_slice(data);
    }

    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.raw[Self::HEAD_SIZE..self.raw_len()]
    }
}

impl Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[package_id: {}; idempotent: {}; raw_len: {}; data: {:?}]", self.package_id(), self.idempotent(), self.raw_len(), self.data())
    }
}

impl Default for Package {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_package {
    use type_layout::TypeLayout;
    use super::Package;

    #[test]
    fn package_info () {
        println!("{}", Package::type_layout());
    }

    #[test]
    fn package_method() {
        let data = "Hello world";

        let mut p1 = Package::new();
        p1.set_package_id(1);
        p1.set_idempotent(2);
        p1.set_data(data.as_bytes());
        p1.active();

        assert_eq!(1, p1.package_id());
        assert_eq!(2, p1.idempotent());
        assert_eq!(Package::HEAD_SIZE + data.len(), p1.raw_len());
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), data);

        let mut buf = bytes::BytesMut::new();
        p1.to_bytes(&mut buf).unwrap();

        assert_eq!(p1.head_code(), p1.raw[0] ^ p1.raw[9]);
        assert_eq!(p1.data_code(), p1.raw[0] ^ p1.raw[Package::HEAD_SIZE + 10]);

        let mut p2 = Package::new();
        p2.from_bytes(&buf).unwrap();

        assert_eq!(p2.package_id(), p1.package_id());
        assert_eq!(p2.idempotent(), p1.idempotent());
        assert_eq!(p2.raw_len(), p1.raw_len());
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), std::str::from_utf8(p2.data()).unwrap());
    }
}
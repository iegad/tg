use std::sync::Arc;
use bytes::{BytesMut, BufMut};
use type_layout::TypeLayout;
use crate::g;
use lazy_static::lazy_static;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};

lazy_static! {
    pub static ref WBUF_POOL: LinearObjectPool<BytesMut> =
        LinearObjectPool::new(|| BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        |v| {
            if v.capacity() < g::DEFAULT_BUF_SIZE {
                v.resize(g::DEFAULT_BUF_SIZE, 0);
            }
            unsafe{ v.set_len(0); }
        });

    pub(crate) static ref PACK_POOL: LinearObjectPool<Package> = LinearObjectPool::new(||Package::new(), |v|v.reset());
    pub static ref REQ_POOL: LinearObjectPool<Package> = LinearObjectPool::new(||Package::new(), |v|v.reset());
}

pub type PackBuf = Arc<LinearReusable<'static, BytesMut>>;

#[repr(C, packed(4))]
#[derive(TypeLayout, Debug)]
struct PackHead {
    serv_pack_id: u32,
    router_id: u32,
    idempotent: u32,
    data_len: u32,
}

unsafe impl Sync for PackHead{}
unsafe impl Send for PackHead{}

impl PackHead {
    fn valid(&self) -> bool {
        self.serv_pack_id > 0 && self.idempotent > 0
    }
}

#[derive(TypeLayout, Debug)]
pub struct Package {
    hbuf: [u8; 16],
    data: Vec::<u8>,

    hbuf_pos: usize,
    data_pos: usize,
}

unsafe impl Sync for Package{}
unsafe impl Send for Package{}

impl Package {
    pub const HEAD_SIZE: usize = std::mem::size_of::<PackHead>();

    pub fn new() -> Self {
        Self { 
            hbuf: [0u8; 16],
            data: Vec::with_capacity(g::DEFAULT_BUF_SIZE),
            hbuf_pos: 0,
            data_pos: 0,
        }
    }

    pub fn with_params(service_id: u16, package_id: u16, router_id: u32, idempotent: u32, data: &[u8]) -> Self {
        let mut res = Self::new();
        res.set_service_id(service_id);
        res.set_package_id(package_id);
        res.set_router_id(router_id);
        res.set_idempotent(idempotent);
        res.set_data(data);
        res
    }

    pub fn reset(&mut self) {
        self.hbuf_pos = 0;
        self.data_pos = 0;
        self.data.clear();
        unsafe {self.data.set_len(0);}
    }

    pub fn to_bytes(&self, buf: &mut BytesMut) -> g::Result<()> {
        let p = unsafe { &*(self.hbuf.as_ptr() as *const PackHead) };

        if !p.valid() {
            return Err(g::Err::PackHeadInvalid);
        }

        buf.put(&self.hbuf[..]);

        if p.data_len > 0 {
            buf.put(&self.data[..]);
        }

        Ok(())
    }

    pub fn from_bytes(&mut self, buf: &[u8]) -> g::Result<usize> {
        let buf_len = buf.len();
        let mut _nleft = 0;
        let mut buf_pos = 0;

        assert!(buf_len > 0);

        if self.hbuf_pos < Self::HEAD_SIZE {
            _nleft = Self::HEAD_SIZE - self.hbuf_pos;

            if buf.len() < _nleft {
                _nleft = buf.len();
            }

            self.hbuf[self.hbuf_pos.._nleft].copy_from_slice(&buf[.._nleft]);
            self.hbuf_pos += _nleft;
            buf_pos += _nleft;
        }

        let head = unsafe {&*(&self.hbuf as *const u8 as *const PackHead)};
        tracing::debug!("head.valid => {}", head.valid());
        tracing::debug!("buf_len[{}] > buf_pos[{}] => {}",buf_len,buf_pos, buf_len > buf_pos);
        tracing::debug!("self.data_pos[{}] < head.data_len[{}] => {}", self.data_pos, head.data_len, self.data_pos < head.data_len as usize);
        if head.valid() && buf_len > buf_pos && self.data_pos < head.data_len as usize {
            _nleft = head.data_len as usize - self.data_pos;
            let n = buf_len - buf_pos;
            if _nleft > n {
                _nleft = n;
            }

            self.data.extend_from_slice(&buf[buf_pos.. buf_pos + _nleft]);
            self.data_pos += _nleft;
            buf_pos += _nleft;
        }

        tracing::debug!("takes: {}", buf_pos);
        Ok(buf_pos)
    }

    pub fn valid(&self) -> bool {
        let head = unsafe {&*(&self.hbuf as *const u8 as *const PackHead)};
        head.valid() && head.data_len as usize == self.data_pos && self.hbuf_pos == Self::HEAD_SIZE
    }

    pub fn set_service_id(&mut self, service_id: u16) {
        assert!(service_id > 0);
        self.hbuf[0..2].copy_from_slice(&service_id.to_le_bytes()[..]);
    }

    pub fn service_id(&self) -> u16 {
         unsafe {*(self.hbuf.as_ptr() as *const u8 as *const u16)}
    }

    pub fn set_package_id(&mut self, package_id: u16) {
        assert!(package_id > 0);
        self.hbuf[2..4].copy_from_slice(&package_id.to_le_bytes()[..]);
    }

    pub fn package_id(&self) -> u16 {
        unsafe {*(self.hbuf.as_ptr().add(2) as *const u8 as *const u16)}
    }

    pub fn set_router_id(&mut self, router_id: u32) {
        self.hbuf[4..8].copy_from_slice(&router_id.to_le_bytes()[..]);
    }

    pub fn router_id(&self) -> u32 {
        unsafe {*(self.hbuf.as_ptr().add(4) as *const u8 as *const u32)}
    }

    pub fn set_idempotent(&mut self, idempotent: u32) {
        assert!(idempotent > 0);
        self.hbuf[8..12].copy_from_slice(&idempotent.to_le_bytes()[..]);
    }

    pub fn idempotent(&self) -> u32 {
        unsafe {*(self.hbuf.as_ptr().add(8) as *const u8 as *const u32)}
    }

    pub fn data_len(&self) -> usize {
        unsafe {*(self.hbuf.as_ptr().add(12) as *const u8 as *const u32) as usize}
    }

    pub fn set_data(&mut self, data: &[u8]) {
        unsafe {self.data.set_len(0);}
        self.data.extend_from_slice(data);
        let dlen = self.data.len() as u32;
        self.hbuf[12..16].copy_from_slice(&dlen.to_le_bytes()[..]);
    }

    pub fn data(&self) -> &[u8] {
        &self.data
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
        p1.set_service_id(1);
        p1.set_package_id(2);
        p1.set_router_id(3);
        p1.set_idempotent(4);
        p1.set_data(data.as_bytes());

        assert_eq!(1, p1.service_id());
        assert_eq!(2, p1.package_id());
        assert_eq!(3, p1.router_id());
        assert_eq!(4, p1.idempotent());
        assert_eq!(11, p1.data_len());
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), data);


        let mut buf = bytes::BytesMut::new();
        p1.to_bytes(&mut buf).unwrap();

        let mut p2 = Package::new();
        p2.from_bytes(&buf).unwrap();

        assert_eq!(p2.service_id(), p1.service_id());
        assert_eq!(p2.package_id(), p1.package_id());
        assert_eq!(p2.router_id(), p1.router_id());
        assert_eq!(p2.idempotent(), p1.idempotent());
        assert_eq!(p2.data_len(), p1.data_len());
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), std::str::from_utf8(p2.data()).unwrap());
    }
}
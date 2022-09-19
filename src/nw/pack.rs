use bytes::{Buf, BufMut, BytesMut};
use lockfree_object_pool::{LinearObjectPool};
use std::{mem::{size_of, MaybeUninit}, sync::Once};
use type_layout::TypeLayout;
use lazy_static::lazy_static;

use crate::g;

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

lazy_static! {
    static ref REQ_POOL: LinearObjectPool<Package> =
        LinearObjectPool::new(|| Package::new(), |v| { v.reset() });
}

impl Package {
    pub const HEAD_SIZE: usize = size_of::<u16>()
        + size_of::<u16>()
        + size_of::<u32>()
        + size_of::<u32>()
        + size_of::<u32>();
    pub const MAX_DATA_SIZE: usize = 1024 * 1024 * 1024;
    const HEAD_16_KEY: u16 = 0x0A0B;
    const HEAD_32_KEY: u32 = 0x0C0D0E0F;

    pub fn req_pool() -> &'static LinearObjectPool<Package> {
        static mut INSTANCE: MaybeUninit<LinearObjectPool<Package>> = MaybeUninit::uninit();
        static ONCE: Once = Once::new();
    
        ONCE.call_once(|| unsafe {
            INSTANCE.as_mut_ptr().write(LinearObjectPool::new(
                || Package::new(),
                |v| v.reset(),
            ));
        });
    
        unsafe { &*INSTANCE.as_ptr()}
    }

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

    pub fn from(rbuf: &mut BytesMut) -> g::Result<Self> {
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
        let data_len = raw_len - Self::HEAD_SIZE;

        let data = if data_len == 0 {
            BytesMut::new()
        } else if data_len <= rbuf.len() {
            rbuf.split_to(data_len)
        } else {
            return Err(g::Err::PackDataSizeInvalid);
        };

        Ok(Self {
            service_id,
            package_id,
            router_id,
            idempotent,
            raw_len,
            data,
        })
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.service_id = 0;
    }

    pub fn from_buf(&mut self, rbuf: &mut BytesMut) -> g::Result<()> {
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
        if self.raw_len > Self::HEAD_SIZE {
            let mut data_len = self.raw_len - Self::HEAD_SIZE;
            if rbuf.len() < data_len {
                data_len = rbuf.len();
            }

            self.data = rbuf.split_to(data_len);
        }

        Ok(())
    }

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

    #[inline(always)]
    pub fn fill_data(&mut self, rbuf: &mut BytesMut) {
        let rbuf_len = rbuf.len();

        let mut left_len = self.raw_len - Self::HEAD_SIZE - self.data.len();
        if left_len > rbuf_len {
            left_len = rbuf_len;
        }

        self.data.extend_from_slice(&rbuf[0..left_len]);
        rbuf.advance(left_len);
    }

    #[inline(always)]
    pub fn append_data(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
        self.raw_len += data.len();
    }

    #[inline(always)]
    pub fn head_valid(&self) -> bool {
        self.service_id > 0 && self.package_id > 0 && self.router_id > 0 && self.idempotent > 0
    }

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
        assert!(data.len() < Self::MAX_DATA_SIZE);

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

    #[inline(always)]
    pub fn parse(rbuf: &mut BytesMut, pack: &mut Self) -> g::Result<()> {
        if rbuf.len() == 0 {
            return Err(g::Err::PackDataSizeInvalid);
        }

        if pack.head_valid() {
            if !pack.valid() {
                pack.fill_data(rbuf);
            }
        } else {
            if rbuf.len() < Self::HEAD_SIZE {
                return Err(g::Err::PackDataSizeInvalid);
            }
            pack.from_buf(rbuf)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod pack_test {
    use bytes::{BufMut, BytesMut};
    use type_layout::TypeLayout;
    use crate::utils;
    use super::Package;

    #[test]
    fn test_package_info() {
        println!("-------------->> layout:\n{}", Package::type_layout());
        println!(
            "-------------->> package size [{}]",
            std::mem::size_of::<Package>()
        );
        let p = Package::with_params(1, 2, 3, 4, b"Hello world");
        println!("{:?}", p);
    }

    #[test]
    fn test_package_performance() {
        let beg = utils::now_unix_nanos();
        for i in 0..1_000_000 {
            let p1 = Package::with_params(1, 2, i + 2, i + 1, b"Hello world");
            let mut buf = p1.to_bytes();
            let p2 = Package::from(&mut buf).unwrap();
            assert_eq!(format!("{:?}", p1), format!("{:?}", p2));
        }
        println!(
            "--->>> 百万级测试用时: {} ns",
            utils::now_unix_nanos() - beg
        );
    }

    #[test]
    fn test_package() {
        let mut p1 = Package::new();
        assert!(!p1.valid());

        p1.set_service_id(1);
        p1.set_package_id(2);
        p1.set_router_id(3);
        p1.set_idempotent(4);
        p1.set_data(b"Hello world");

        assert!(p1.valid());
        assert_eq!(p1.service_id(), 1);
        assert_eq!(p1.package_id(), 2);
        assert_eq!(p1.router_id(), 3);
        assert_eq!(p1.data().len(), 11);
        assert_eq!(p1.raw_len(), 27);
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), "Hello world");

        let p2 = Package::with_params(1, 2, 3, 4, b"Hello world");
        assert!(p2.valid());
        assert_eq!(p2.service_id(), 1);
        assert_eq!(p2.package_id(), 2);
        assert_eq!(p2.router_id(), 3);
        assert_eq!(p2.data().len(), 11);
        assert_eq!(p2.raw_len(), 27);
        assert_eq!(std::str::from_utf8(p2.data()).unwrap(), "Hello world");

        let mut iobuf = BytesMut::with_capacity(1500);

        let buf = p1.to_bytes();
        iobuf.put(&buf[..]);
        println!("+++ start pos: {:p}", &iobuf[0]);

        println!(">>>> before ----- CAP: {}", iobuf.capacity());
        assert_eq!(buf.len(), 27);

        let p3 = Package::from(&mut iobuf).unwrap();

        iobuf.put_bytes(0, 1);
        println!(">>>> after ----- CAP: {}", iobuf.capacity());
        println!("+++ start pos: {:p}", &iobuf[0]);
        assert!(p3.valid());
        assert_eq!(p3.service_id(), 1);
        assert_eq!(p3.package_id(), 2);
        assert_eq!(p3.router_id(), 3);
        assert_eq!(p3.data().len(), 11);
        assert_eq!(p3.raw_len(), 27);
        assert_eq!(std::str::from_utf8(p3.data()).unwrap(), "Hello world");
    }
}

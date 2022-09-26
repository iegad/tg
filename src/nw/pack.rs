use crate::g;
use bytes::{Buf, BufMut, BytesMut};
use lazy_static::lazy_static;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use std::sync::Arc;
use type_layout::TypeLayout;

// ---------------------------------------------- static variables ----------------------------------------------
//
//
lazy_static! {
    /// # PACK_POOL
    ///
    /// for internal to use
    pub(crate) static ref PACK_POOL: LinearObjectPool<Package> = LinearObjectPool::new(|| Package::new(), |v| { v.reset() });

    /// # REQ_POOL
    ///
    /// you can pull one from package's pool when need a package for request.
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::REQ_POOL;
    /// use tg::nw::pack::Package;
    ///
    /// let data = "Hello world";
    /// let mut req = REQ_POOL.pull();
    /// req.set_service_id(1);
    /// req.set_package_id(2);
    /// req.set_data(data.as_bytes());
    /// assert_eq!(req.service_id(), 1);
    /// assert_eq!(req.package_id(), 2);
    /// assert_eq!(req.data_len(), data.len());
    /// ```
    pub static ref REQ_POOL: LinearObjectPool<Package> =
        LinearObjectPool::new(|| Package::new(), |v| { v.reset() });

    /// # WBUF_POOL
    ///
    /// you can pull one from BytesMut's pool when need a BytesMut for write buffer.
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::WBUF_POOL;
    /// use bytes::BufMut;
    ///
    /// let mut wbuf = WBUF_POOL.pull();
    /// wbuf.put("Hello world".as_bytes());
    /// assert_eq!(wbuf.len(), 11);
    /// ```
    pub static ref WBUF_POOL: LinearObjectPool<BytesMut> =
        LinearObjectPool::new(|| BytesMut::with_capacity(g::DEFAULT_BUF_SIZE),
        |v| {
            if v.capacity() < g::DEFAULT_BUF_SIZE {
                v.resize(g::DEFAULT_BUF_SIZE, 0);
            }
            unsafe{ v.set_len(0); }
        });
}

// ---------------------------------------------- pack::Response ----------------------------------------------
//
//
/// # Response
///
/// network's event process reply to connection data
pub type Response = Arc<LinearReusable<'static, BytesMut>>;

// ---------------------------------------------- pack::Package ----------------------------------------------
//
//
/// # Package
///
/// for network transport
///
/// windows: sizeof(Package) is 56
///
/// unix: sizeof(Pakcage) is TODO
///
/// [Package] composit by Head and Body
///
/// # Head:
///
/// Head's size is 16 Bytes.
///
/// `service_id`: 16bit: match service.
///
/// `package_id`: 16bit: match service's implement handler.
///
/// `router_id`:  32bit: match service's node when service has multiple nodes.
///
/// `idempotent`: 32bit: check if the package has already been processed.
///
/// `data_len`:    32bit: Body's size.
///
/// # Memory Layout
///
/// | service_id: `2 Bytes` | package_id: `2 Bytes` | router_id: `4 Bytes` | idempotent: `4 Bytes` | data_len: `4 Bytes` | data: `[0, 1G Bytes]`
///
/// # Example
///
/// ```
/// use tg::nw::pack::Package;
///
/// let p1 = Package::new();
/// assert_eq!(0, p1.data_len());
///
/// let data = "Hello world";
/// let p2 = Package::with_params(1, 2, 3, 4, data.as_bytes());
/// assert_eq!(p2.service_id(), 1);
/// assert_eq!(p2.package_id(), 2);
/// assert_eq!(p2.router_id(), 3);
/// assert_eq!(p2.idempotent(), 4);
/// assert_eq!(p2.data_len(), data.len());
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
    data_len: usize,
    data: BytesMut,
}

impl Package {
    /// # Package::HEAD_SIZE
    ///
    /// Package head's size
    ///
    /// ```
    /// assert_eq!(tg::nw::pack::Package::HEAD_SIZE, 16);
    /// ```
    pub const HEAD_SIZE: usize = 16;

    /// # Package::MAX_DATA_SIZE
    ///
    /// Package's data max size: 1G
    pub const MAX_DATA_SIZE: usize = 1024 * 1024 * 1024;

    /// 16bit KEY use for Package's field 2 bytes encrypt/decrypt
    const HEAD_16_KEY: u16 = 0xFA0B;

    /// 32bit KEY use for Package's field 4 bytes encrypt/decrypt
    const HEAD_32_KEY: u32 = 0xFC0DFE0F;

    /// # Package::new
    ///
    /// build a default Package.
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::Package;
    ///
    /// let p = Package::new();
    /// assert_eq!(p.data_len(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            service_id: 0,
            package_id: 0,
            router_id: 0,
            idempotent: 0,
            data_len: 0,
            data: BytesMut::new(),
        }
    }

    /// # Package::with_params
    ///
    /// build a package with fields
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::Package;
    ///
    /// let p = Package::with_params(1, 2, 3, 4, "Hello world".as_bytes());
    /// assert_eq!(p.service_id(), 1);
    /// assert_eq!(p.package_id(), 2);
    /// assert_eq!(p.router_id(), 3);
    /// assert_eq!(p.idempotent(), 4);
    /// assert_eq!(p.data_len(), 11);
    /// assert_eq!(std::str::from_utf8(p.data()).unwrap(), "Hello world");
    /// ```
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
            data_len: data.len(),
            data: BytesMut::from(data),
        }
    }

    /// # Package::parse
    ///
    /// deserialize pack from rbuf.
    ///
    /// # Return
    ///
    /// if success returns Ok(()), else returns Err(err);
    ///
    /// # Notes
    ///
    /// 1. If the `rbuf` is not sufficient to deserialize a complete `pack`,
    ///    the `pack` will be in a `half-loaded` state, pack.valid() is false but pack.head_valid() is true
    ///
    /// 2. when `!pack.valid() && !pack.head_valid()` must `rbuf.len()` >= [Package::HEAD_SIZE]
    ///
    /// 3. when `!pack.valid() && pack.head_valid()` must `rbuf.len()` > 0
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::WBUF_POOL;
    /// use tg::nw::pack::REQ_POOL;
    /// use tg::nw::pack::Package;
    ///
    /// let mut buf = WBUF_POOL.pull();
    /// let p1 = Package::with_params(1, 2, 3, 4, "Hello world".as_bytes());
    /// p1.to_bytes(&mut buf);
    ///
    /// let mut p2 = REQ_POOL.pull();
    /// assert_eq!(Package::parse(&mut *buf, &mut p2).unwrap(), ());
    ///
    /// assert_eq!(format!("{p1}"), format!("{&*p2}"));
    /// ```
    pub fn parse(rbuf: &mut BytesMut, pack: &mut Self) -> g::Result<()> {
        debug_assert!(!pack.valid());

        if pack.head_valid() {
            if rbuf.len() == 0 {
                return Err(g::Err::BufSizeInvalid);
            }

            pack.fill_data(rbuf);
            return Ok(());
        }

        if rbuf.len() < Self::HEAD_SIZE {
            return Err(g::Err::BufSizeInvalid);
        }

        pack.from_buf(rbuf)
    }

    /// # Package::from_bytes
    ///
    /// deserialize Package from `rbuf`
    ///
    /// # Returns
    ///
    /// If successful returns Ok(Package) else returns Err(err)
    ///
    /// # Notes
    ///
    /// `rbuf` must contains a complete package.
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::Package;
    /// use tg::nw::pack::WBUF_POOL;
    ///
    /// let p1 = Package::with_params(1, 2, 3, 4, "Hello world".as_bytes());
    /// let mut buf = WBUF_POOL.pull();
    /// p1.to_bytes(&mut buf);
    ///
    /// let p2 = Package::from_bytes(&mut buf).unwrap();
    /// assert_eq!(format!("{p1}", p1), format!("{p2}", p2));
    /// ```
    ///
    /// TODO: 实际上, 该函数没并有任何地方使用.
    pub fn from_bytes(rbuf: &mut BytesMut) -> g::Result<Self> {
        if rbuf.len() < Self::HEAD_SIZE {
            return Err(g::Err::PackHeadInvalid("Head size is invalid"));
        }

        let service_id = rbuf.get_u16_le() ^ Self::HEAD_16_KEY;
        if service_id == 0 {
            return Err(g::Err::PackHeadInvalid("service_id is invalid"));
        }

        let package_id = rbuf.get_u16_le() ^ Self::HEAD_16_KEY;
        if package_id == 0 {
            return Err(g::Err::PackHeadInvalid("package_id is invalid"));
        }

        let router_id = rbuf.get_u32_le() ^ Self::HEAD_32_KEY;

        let idempotent = rbuf.get_u32_le() ^ Self::HEAD_32_KEY;
        if idempotent == 0 {
            return Err(g::Err::PackHeadInvalid("idempotent is invalid"));
        }

        let data_len = (rbuf.get_u32_le() ^ Self::HEAD_32_KEY) as usize;
        if data_len > Self::MAX_DATA_SIZE {
            return Err(g::Err::PackDataSizeInvalid);
        }

        if data_len > rbuf.len() {
            return Err(g::Err::PackDataSizeInvalid);
        }

        Ok(Self {
            service_id,
            package_id,
            router_id,
            idempotent,
            data_len,
            data: rbuf.split_to(data_len),
        })
    }

    /// # Package.from_bytes
    ///
    /// deserialize package from `rbuf`
    ///
    /// it's a internal method, called by [Package::parse]
    ///
    /// this method will reset the package when it's complete or half-loaded.
    ///
    /// # Returns
    ///
    /// when rbuf contains a complete package buffer, the package will be deserialize a complete package.
    ///
    /// when `rbuf.len` < Package::HEAD_SIZE returns error.
    ///
    /// when `rbuf.len` >= Package::HEAD_SIZE && rbuf not contains a complete package: returns half-loaded package.
    ///
    /// when rbuf's data is invalid returns error.
    fn from_buf(&mut self, rbuf: &mut BytesMut) -> g::Result<()> {
        if rbuf.len() < Self::HEAD_SIZE {
            return Err(g::Err::PackHeadInvalid("Head size is invalid"));
        }

        self.service_id = rbuf.get_u16_le() ^ Self::HEAD_16_KEY;
        if self.service_id == 0 {
            return Err(g::Err::PackHeadInvalid("service_id is invalid"));
        }

        self.package_id = rbuf.get_u16_le() ^ Self::HEAD_16_KEY;
        if self.package_id == 0 {
            return Err(g::Err::PackHeadInvalid("package_id is invalid"));
        }

        self.router_id = rbuf.get_u32_le() ^ Self::HEAD_32_KEY;

        self.idempotent = rbuf.get_u32_le() ^ Self::HEAD_32_KEY;
        if self.idempotent == 0 {
            return Err(g::Err::PackHeadInvalid("idempotent is invalid"));
        }

        self.data_len = (rbuf.get_u32_le() ^ Self::HEAD_32_KEY) as usize;
        if self.data_len > Self::MAX_DATA_SIZE {
            return Err(g::Err::PackDataSizeInvalid);
        }

        if self.data_len > 0 {
            let rbuf_len = rbuf.len();
            let mut actual_len = self.data_len;
            if rbuf_len < actual_len {
                actual_len = rbuf_len;
            }

            self.data = rbuf.split_to(actual_len);
        }

        Ok(())
    }

    /// # Package.fill_data
    ///
    /// fill the half-loaded package, let it be complete.
    ///
    /// it's a internal method, called by [Package::parse].
    ///
    /// call this method, package must be half-loaded state.
    fn fill_data(&mut self, rbuf: &mut BytesMut) {
        debug_assert!(self.head_valid());

        let rbuf_len = rbuf.len();

        let mut left_len = self.data_len - self.data.len();
        if left_len > rbuf_len {
            left_len = rbuf_len;
        }

        self.data.extend_from_slice(&rbuf[0..left_len]);
        rbuf.advance(left_len);
    }

    /// # Package.reset
    ///
    /// reset the package, package.valid() == false, package.head_valid() == false, package.raw_len == Package::HEAD_SIZE;
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::Package;
    ///
    /// let mut p = Package::with_params(1, 2, 3, 4, "Hello world".as_bytes());
    /// assert!(p.valid() && p.head_valid());
    /// p.reset();
    /// assert!(!p.valid() && !p.head_valid());
    /// ```
    #[inline]
    pub fn reset(&mut self) {
        self.service_id = 0;
        unsafe {
            self.data.set_len(0);
        }
        self.data_len = 0;
    }

    /// # Package.to_bytes
    ///
    /// serialize package to `wbuf`
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::Package;
    /// use bytes::BytesMut;
    ///
    /// let p = Package::with_params(1, 2, 3, 4, "Hello world".as_bytes());
    /// let mut wbuf = BytesMut::new();
    /// p.to_bytes(&mut wbuf);
    /// assert_eq!(wbuf.len(), Package::HEAD_SIZE + 11);
    /// ```
    #[inline]
    pub fn to_bytes(&self, wbuf: &mut BytesMut) {
        debug_assert!(self.valid(), "to_bytes must called by a valid package.");

        wbuf.put_u16_le(self.service_id ^ Self::HEAD_16_KEY);
        wbuf.put_u16_le(self.package_id ^ Self::HEAD_16_KEY);
        wbuf.put_u32_le(self.router_id ^ Self::HEAD_32_KEY);
        wbuf.put_u32_le(self.idempotent ^ Self::HEAD_32_KEY);
        wbuf.put_u32_le((self.data_len as u32) ^ Self::HEAD_32_KEY);
        if self.data_len > 0 {
            wbuf.put(&self.data[..]);
        }
    }

    /// # Package.append_data
    ///
    /// Append data to `package.data` tail, but `package.data.len() <= Package::MAX_DATA_SIZE`.
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::Package;
    ///
    /// let mut p = Package::with_params(1, 2, 3, 4, "Hello".as_bytes());
    /// assert_eq!(std::str::from_utf8(p.data()).unwrap(), "Hello");
    /// assert_eq!(p.data_len(), 5);
    ///
    /// p.append_data(" world".as_bytes());
    /// assert_eq!(std::str::from_utf8(p.data()).unwrap(), "Hello world");
    /// assert_eq!(p.data_len(), 11);
    /// ```
    #[inline]
    pub fn append_data(&mut self, data: &[u8]) -> g::Result<()> {
        if self.data_len + data.len() > Self::MAX_DATA_SIZE {
            return Err(g::Err::PackTooLong);
        }

        self.data.extend_from_slice(data);
        Ok(self.data_len += data.len())
    }

    /// # Package.head_valid
    ///
    /// check the package's head is valid.
    ///
    /// # Returns
    ///
    /// If package's head is valid returns true, else returns false.
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::Package;
    ///
    /// let mut p = Package::new();
    /// assert!(!p.head_valid());
    ///
    /// p.set_service_id(1);
    /// p.set_package_id(2);
    /// p.set_router_id(3);
    /// p.set_idempotent(4);
    ///
    /// assert!(p.head_valid());
    /// ```
    #[inline]
    pub fn head_valid(&self) -> bool {
        self.service_id > 0 && self.package_id > 0 && self.idempotent > 0
    }

    /// # Package.valid
    ///
    /// check the package is valid
    ///
    /// # Returns
    ///
    /// If package is valid returns true, else returns false.
    ///
    /// # Notes
    ///
    /// If `package.valid()` means package is complete.
    ///
    /// If `!package.valid() && package.head_valid()` means package is half-loaded.
    ///
    /// If `!package.valid() && !package.head_valid()` means package is invalid.
    ///
    /// # Example
    ///
    /// ```
    /// use tg::nw::pack::Package;
    ///
    /// let mut p = Package::new();
    /// assert!(!p.head_valid());
    ///
    /// p.set_service_id(1);
    /// p.set_package_id(2);
    /// p.set_router_id(3);
    /// p.set_idempotent(4);
    ///
    /// assert!(p.head_valid() && p.valid());
    ///
    /// p.set_data("12345".as_bytes());
    /// assert!(p.head_valid() && p.valid());
    /// ```
    #[inline]
    pub fn valid(&self) -> bool {
        self.data_len == self.data.len() && self.head_valid()
    }

    /// set package's service_id
    #[inline]
    pub fn set_service_id(&mut self, service_id: u16) {
        self.service_id = service_id;
    }

    /// get package's service_id
    #[inline]
    pub fn service_id(&self) -> u16 {
        self.service_id
    }

    /// set package's package_id
    #[inline]
    pub fn set_package_id(&mut self, package_id: u16) {
        self.package_id = package_id;
    }

    /// get package's package_id
    #[inline]
    pub fn package_id(&self) -> u16 {
        self.package_id
    }

    /// set package's router_id
    #[inline]
    pub fn set_router_id(&mut self, router_id: u32) {
        self.router_id = router_id;
    }

    /// get package's router_id
    #[inline]
    pub fn router_id(&self) -> u32 {
        self.router_id
    }

    /// set package's idempotent
    #[inline]
    pub fn set_idempotent(&mut self, idempotent: u32) {
        self.idempotent = idempotent;
    }

    /// get package's idempotent
    #[inline]
    pub fn idempotent(&self) -> u32 {
        self.idempotent
    }

    /// set package's data
    #[inline]
    pub fn set_data(&mut self, data: &[u8]) {
        debug_assert!(data.len() <= Self::MAX_DATA_SIZE);

        self.data.clear();
        self.data.extend_from_slice(data);

        self.data_len = data.len();
    }

    /// get package's data
    #[inline]
    pub fn data(&self) -> &[u8] {
        &self.data[..self.data_len]
    }

    /// set package's raw length, `raw_len == Package::HEAD_SIZE + package.data.len()`.
    #[inline]
    pub fn data_len(&self) -> usize {
        self.data_len
    }
}

// ---------------------------------------------- UNIT TEST ----------------------------------------------
//
//
#[cfg(test)]
mod pack_test {
    use crate::nw::pack::{self, WBUF_POOL};

    use super::Package;
    use bytes::{BufMut, BytesMut};
    use type_layout::TypeLayout;

    /// Package information
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
        assert_eq!(p1.data_len(), data.len());
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), data);

        let p2 = Package::with_params(1, 2, 3, 4, data.as_bytes());
        assert!(p2.valid());
        assert_eq!(p2.service_id(), 1);
        assert_eq!(p2.package_id(), 2);
        assert_eq!(p2.router_id(), 3);
        assert_eq!(p2.data().len(), data.len());
        assert_eq!(p2.data_len(), data.len());
        assert_eq!(std::str::from_utf8(p2.data()).unwrap(), data);

        let mut iobuf = BytesMut::with_capacity(1500);
        let mut buf = WBUF_POOL.pull();
        p1.to_bytes(&mut buf);
        iobuf.put(&buf[..]);
        let iobuf_pos = &iobuf[0] as *const u8;
        assert_eq!(iobuf.capacity(), 1500);
        assert_eq!(iobuf.len(), buf.len());
        assert_eq!(buf.len(), data.len() + pack::Package::HEAD_SIZE);

        let p3 = Package::from_bytes(&mut iobuf).unwrap();
        assert_eq!(iobuf.capacity(), 1500 - buf.len());

        iobuf.put_bytes(0, 1);
        let iobuf_pos_ = &iobuf[0] as *const u8;
        assert_eq!(iobuf_pos_ as usize - iobuf_pos as usize, buf.len());

        assert!(p3.valid());
        assert_eq!(p3.service_id(), 1);
        assert_eq!(p3.package_id(), 2);
        assert_eq!(p3.router_id(), 3);
        assert_eq!(p3.data().len(), data.len());
        assert_eq!(p3.data_len(), data.len());
        assert_eq!(std::str::from_utf8(p3.data()).unwrap(), data);

        let mut p4 = Package::new();
        p3.to_bytes(&mut buf);
        p4.from_buf(&mut buf).unwrap();

        assert!(p4.valid());
        assert_eq!(p4.service_id(), 1);
        assert_eq!(p4.package_id(), 2);
        assert_eq!(p4.router_id(), 3);
        assert_eq!(p4.data().len(), data.len());
        assert_eq!(p4.data_len(), data.len());
        assert_eq!(std::str::from_utf8(p4.data()).unwrap(), data);

        let mut p5 = Package::new();
        p4.to_bytes(&mut buf);
        assert_eq!(Package::parse(&mut buf, &mut p5).unwrap(), ());
    }
}

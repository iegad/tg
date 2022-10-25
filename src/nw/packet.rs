use std::fmt::Display;

use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use crate::g;

pub type LinearItem = LinearReusable<'static, Packet>;

lazy_static::lazy_static! {
    /// packet 内部使用对象池.
    static ref INNER_POOL: LinearObjectPool<Packet> = LinearObjectPool::new(Packet::default, |v|{v.reset();});

    pub static ref WBUF_POOL: LinearObjectPool<Vec<u8>> = LinearObjectPool::new(|| vec![0; g::DEFAULT_BUF_SIZE], |v| unsafe { v.set_len(g::DEFAULT_BUF_SIZE); });
    pub static ref REQ_POOL: LinearObjectPool<Packet> = LinearObjectPool::new(Packet::new, |v|{v.reset();});
}

pub struct Packet(Vec<u8>);

impl Packet {
    pub const HEAD_SIZE: usize = 12;
    pub const MAX_DATA_SIZE: usize = 1024 * 1024 * 2;

    #[inline(always)]
    pub fn new() -> Self {
        Self(Vec::with_capacity(g::DEFAULT_BUF_SIZE))
    }

    #[allow(clippy::uninit_vec)]
    pub fn with(id: u16, idempotent: u32, data: &[u8]) -> Self {
        let dlen = data.len();
        assert!(dlen <= Self::MAX_DATA_SIZE);

        let rlen = Self::HEAD_SIZE + dlen;
        let mut raw = Vec::<u8>::with_capacity(rlen);

        unsafe {
            raw.set_len(rlen);
            let p = raw.as_mut_ptr();
            *(p.add(2) as *mut u16) = id;
            *(p.add(4) as *mut u32) = idempotent;
            *(p.add(8) as *mut u32) = rlen as u32;

            std::ptr::copy_nonoverlapping(data.as_ptr(), raw.as_mut_ptr().add(Self::HEAD_SIZE), dlen);
        }

        raw[0] = raw[2] ^ raw[11];
        raw[1] = raw[2] ^ raw[rlen - 1];
        Self(raw)
    }

    #[inline(always)]
    pub fn id(&self) -> u16 {
        unsafe { *(self.0.as_ptr().add(2) as *const u16) }
    }

    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        unsafe { *(self.0.as_ptr().add(4) as *const u32) }
    }

    #[inline(always)]
    pub fn raw_len(&self) -> usize {
        unsafe { *(self.0.as_ptr().add(8) as *const u32) as usize }
    }

    #[inline(always)]
    pub fn raw(&self) -> &[u8] {
        &self.0
    }

    #[inline(always)]
    pub fn data(&self) -> &[u8] {
        &self.0[Self::HEAD_SIZE..]
    }

    #[inline(always)]
    pub fn valid(&self) -> bool {
        let rlen = self.0.len();

        unsafe {
            let p = self.0.as_ptr();
            *(p.add(2) as *const u16) > 0 
            && *(p.add(4) as *const u32) > 0 
            && *(p.add(8) as *const u32) as usize == rlen
            && *p == *p.add(2) ^ *p.add(11)
            && *p.add(1) == *p.add(2) ^ *p.add(rlen - 1)
        }
    }
    
    #[inline(always)]
    pub fn set_id(&mut self, id: u16) {
        unsafe { *(self.0.as_mut_ptr().add(2) as *mut u16) = id }
    }

    #[inline(always)]
    pub fn set_idempotent(&mut self, idempotent: u32) {
        unsafe { *(self.0.as_mut_ptr().add(4) as *mut u32) = idempotent }
    }

    #[inline(always)]
    pub fn set_data(&mut self, data: &[u8]) {
        let dlen = data.len(); 
        if dlen == 0 {
            return;
        }

        assert!(dlen <= Self::MAX_DATA_SIZE);

        let rlen = Self::HEAD_SIZE + dlen;
        if self.0.capacity() < rlen {
            self.0.reserve(rlen);
        }

        unsafe {
            self.0.set_len(rlen);
            let p = self.0.as_mut_ptr();
            std::ptr::copy_nonoverlapping(data.as_ptr(), p.add(12), dlen);
            *(p.add(8) as *mut u32) = rlen as u32;
        }
    }

    #[inline(always)]
    pub fn setup(&mut self) {
        self.0[0] = self.0[2] ^ self.0[11];
        self.0[1] = self.0[2] ^ self.0[self.raw_len() - 1];
    }

    #[inline(always)]
    pub fn set(&mut self, id: u16, idempotent: u32, data: &[u8]) {
        let dlen = data.len();
        assert!(dlen <= Self::MAX_DATA_SIZE);

        let rl = Self::HEAD_SIZE + dlen;
        if self.0.capacity() < rl {
            self.0.reserve(rl);
        }

        unsafe {
            self.0.set_len(rl);
            let p = self.0.as_mut_ptr();

            *(p.add(2) as *mut u16) = id;
            *(p.add(4) as *mut u32) = idempotent;
            *(p.add(8) as *mut u32) = rl as u32;

            std::ptr::copy_nonoverlapping(data.as_ptr(), p.add(Self::HEAD_SIZE), dlen);
        }

        self.0[0] = self.0[2] ^ self.0[11];
        self.0[1] = self.0[2] ^ self.0[rl - 1];
    }

    pub(crate) fn reset(&mut self) {
        unsafe { self.0.set_len(0); }
    }

    #[inline(always)]
    pub(crate) fn check_hc(&self) -> bool {
        let p = self.0.as_ptr();
        unsafe { *p == *p.add(2) ^ *p.add(11) }
    }

    #[inline(always)]
    pub(crate) fn check_rc(&self) -> bool {
        let p = self.0.as_ptr();
        unsafe { *p.add(1) == *p.add(2) ^ *p.add(self.0.len() - 1) }
    }
}

impl From<Vec<u8>> for Packet {
    fn from(raw: Vec<u8>) -> Self {
        Self(raw)
    }
}

impl From<&[u8]> for Packet {
    fn from(raw: &[u8]) -> Self {
        Self(raw.to_vec())
    }
}

impl Default for Packet {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for Packet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Packet[id: {}, idempotent: {}, raw_len: {}, data: {}]", self.id(), self.idempotent(), self.raw_len(), hex::encode(self.data()))
    }
}

pub struct Builder {
    current: Option<LinearReusable<'static, Packet>>,
    pos: usize
}

impl Builder {
    pub fn new() -> Self {
        Self {
            current: None,
            pos: 0,
        }
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.pos = 0;
        if let Some(v) = self.current.as_deref_mut() {
            v.reset();
        }
    }

    pub fn parse(&mut self, buf: &[u8]) -> g::Result<Option<(LinearItem, bool)>> {
        let mut buflen = buf.len();
        assert!(buflen > 0);

        // step 1: 确认 buf 可读位置和 未读长度
        let mut bufpos = self.pos;
        buflen -= bufpos;

        if buflen == 0 {
            return Ok(None);
        }

        let mut current = match self.current.take() {
            Some(v) => v,
            None => INNER_POOL.pull(),
        };

        let mut current_len = current.0.len();

        // 解析消息头
        if current_len < Packet::HEAD_SIZE {
            let mut nleft = Packet::HEAD_SIZE - current_len;
            if nleft > buflen {
                nleft = buflen;
            }

            unsafe {
                std::ptr::copy_nonoverlapping(buf.as_ptr().add(bufpos), current.0.as_mut_ptr().add(current_len), nleft);

                bufpos += nleft;
                buflen -= nleft;
                current_len += nleft;
                current.0.set_len(current_len);
            }

            // 检查消息头, 如果检查失败, 直接将失败的结果返回
            if current_len == Packet::HEAD_SIZE && (current.id() == 0 || current.idempotent() == 0 || !current.check_hc()) {
                return Err(g::Err::PackHeadInvalid);
            }
        }

        // 解析消息体
        if buflen > 0 && current_len >= Packet::HEAD_SIZE {
            let rl = current.raw_len();
            if current.0.capacity() < rl {
                current.0.reserve(rl);
            }

            let mut nleft = rl - current_len;
            if nleft > buflen {
                nleft = buflen;
            }

            unsafe {
                std::ptr::copy_nonoverlapping(buf.as_ptr().add(bufpos), current.0.as_mut_ptr().add(current_len), nleft);

                bufpos += nleft;
                buflen -= nleft;
                current_len += nleft;
                current.0.set_len(current_len);
            }
        }

        if buflen == 0 {
            self.pos = 0;
        } else {
            self.pos = bufpos;
        }

        // 检查消息体
        if current_len == current.raw_len() {
            if !current.check_rc() {
                return Err(g::Err::PacketInvalid)
            }

            return if buflen == 0 { Ok(Some((current, false))) } else { Ok(Some((current, true))) }
        }
        
        self.current = Some(current);
        Ok(None)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_packet {
    use super::{Packet, Builder};

    #[test]
    fn packet_basic() {
        let data = "Hello world".as_bytes();

        let mut pkt = Packet::default();
        pkt.set(1, 2, data);
        assert!(pkt.valid());

        assert_eq!(pkt.id(), 1);
        assert_eq!(pkt.idempotent(), 2);
        assert_eq!(pkt.raw_len(), Packet::HEAD_SIZE + 11);
        assert_eq!(std::str::from_utf8(data).unwrap(), std::str::from_utf8(pkt.data()).unwrap());
        assert_eq!(pkt.raw().len(), pkt.raw_len());
        assert!(pkt.check_hc());
        assert!(pkt.check_rc());
    }

    #[test]
    fn packet_build() {
        crate::utils::init_log();

        let data = "Hello world".as_bytes();
        let mut buf = Vec::<u8>::new();

        for i in 0..100 {
            let pkt = Packet::with(1, i + 1, data);
            assert!(pkt.valid());
            assert_eq!(pkt.id(), 1);
            assert_eq!(pkt.idempotent(), i + 1);
            assert_eq!(pkt.raw_len(), Packet::HEAD_SIZE + 11);
            assert_eq!(std::str::from_utf8(data).unwrap(), std::str::from_utf8(pkt.data()).unwrap());
            assert_eq!(pkt.raw().len(), pkt.raw_len());
            assert!(pkt.check_hc());
            assert!(pkt.check_rc());

            buf.extend_from_slice(pkt.raw());
        }

        let mut builder = Builder::new();

        for i in 0..1000 {
            let option_pkt = match builder.parse(&mut buf) {
                Err(err) => panic!("{err}"),
                Ok(v) => v,
            };

            let (pkt, next) = match option_pkt {
                None => break,
                Some((pkt, v)) => (pkt, v),
            };

            tracing::debug!("-->>> idempotent: {}", pkt.idempotent());
            assert_eq!(pkt.id(), 1);
            assert_eq!(pkt.idempotent(), i + 1);
            assert_eq!(hex::encode(data), hex::encode(pkt.data()));
            assert_eq!(pkt.raw().len(), pkt.raw_len());

            if !next {
                assert_eq!(i, 99);
                break;
            }
        }
            
    }
}
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use crate::g;

pub type LinearItem = LinearReusable<'static, Packet>;

lazy_static::lazy_static! {
    static ref RAW_POOL: LinearObjectPool<Packet> = LinearObjectPool::new(Packet::new, |v|{v.reset();});

    pub static ref WBUF_POOL: LinearObjectPool<Vec<u8>> = LinearObjectPool::new(|| vec![0; g::DEFAULT_BUF_SIZE], |v| unsafe { v.set_len(g::DEFAULT_BUF_SIZE); });
    pub static ref REQ_POOL: LinearObjectPool<Packet> = LinearObjectPool::new(Packet::new, |v|{v.reset();});
}

pub enum PacketResult {
    None,
    Next(LinearItem),
    Last(LinearItem),
    Err(g::Err),
}

pub struct Packet(Vec<u8>);

impl Packet {
    pub const HEAD_SIZE: usize = 12;
    pub const MAX_DATA_SIZE: usize = 1024 * 1024 * 2;

    pub fn new() -> Self {
        Self(Vec::with_capacity(g::DEFAULT_BUF_SIZE))
    }

    pub fn with(id: u16, idempotent: u32, data: &[u8]) -> Self {
        let dl = data.len();
        assert!(dl <= Self::MAX_DATA_SIZE);

        let rl = Self::HEAD_SIZE + dl;
        let mut raw = Vec::<u8>::with_capacity(rl);

        unsafe {
            raw.set_len(rl);
            let p = raw.as_mut_ptr();
            *(p.add(2) as *mut u16) = id;
            *(p.add(4) as *mut u32) = idempotent;
            *(p.add(8) as *mut u32) = rl as u32;

            std::ptr::copy(data.as_ptr(), raw.as_mut_ptr().add(Self::HEAD_SIZE), dl);
        }

        raw[0] = raw[2] ^ raw[11];
        raw[1] = raw[2] ^ raw[rl - 1];
        Self(raw)
    }

    pub fn id(&self) -> u16 {
        unsafe { *(self.0.as_ptr().add(2) as *const u16) }
    }

    pub fn idempotent(&self) -> u32 {
        unsafe { *(self.0.as_ptr().add(4) as *const u32) }
    }

    pub fn raw_len(&self) -> usize {
        unsafe { *(self.0.as_ptr().add(8) as *const u32) as usize }
    }

    pub fn raw(&self) -> &[u8] {
        &self.0
    }

    pub fn data(&self) -> &[u8] {
        &self.0[Self::HEAD_SIZE..]
    }

    pub fn valid(&self) -> bool {
        let rl = self.0.len();

        unsafe {
            let p = self.0.as_ptr();
            *(p.add(2) as *const u16) > 0 
            && *(p.add(4) as *const u32) > 0 
            && *(p.add(8) as *const u32) as usize == rl
            && *p == *p.add(2) ^ *p.add(11)
            && *p.add(1) == *p.add(2) ^ *p.add(rl - 1)
        }
    }
    
    pub fn set_id(&mut self, id: u16) {
        unsafe { *(self.0.as_mut_ptr().add(2) as *mut u16) = id }
    }

    pub fn set_idempotent(&mut self, idempotent: u32) {
        unsafe { *(self.0.as_mut_ptr().add(4) as *mut u32) = idempotent }
    }

    pub fn set_data(&mut self, data: &[u8]) {
        let dl = data.len(); 
        if dl == 0 {
            return;
        }

        assert!(dl <= Self::MAX_DATA_SIZE);

        let rl = Self::HEAD_SIZE + dl;
        if self.0.capacity() < rl {
            self.0.reserve(rl);
        }

        unsafe {
            self.0.set_len(rl);
            let p = self.0.as_mut_ptr();
            std::ptr::copy(data.as_ptr(), p.add(12), dl);
            *(p.add(8) as *mut u32) = rl as u32;
        }
    }

    pub fn setup(&mut self) {
        self.0[0] = self.0[2] ^ self.0[11];
        self.0[1] = self.0[2] ^ self.0[self.raw_len() - 1];
    }

    pub fn set(&mut self, id: u16, idempotent: u32, data: &[u8]) {
        let dl = data.len();
        assert!(dl <= Self::MAX_DATA_SIZE);

        let rl = Self::HEAD_SIZE + dl;
        if self.0.capacity() < rl {
            self.0.reserve(rl);
        }

        unsafe {
            self.0.set_len(rl);
            let p = self.0.as_mut_ptr();

            *(p.add(2) as *mut u16) = id;
            *(p.add(4) as *mut u32) = idempotent;
            *(p.add(8) as *mut u32) = rl as u32;

            std::ptr::copy(data.as_ptr(), p.add(12), dl);
        }

        self.0[0] = self.0[2] ^ self.0[11];
        self.0[1] = self.0[2] ^ self.0[rl - 1];
    }

    pub fn reset(&mut self) {
        unsafe { self.0.set_len(0); }
    }

    pub(crate) fn check_hc(&self) -> bool {
        let p = self.0.as_ptr();
        unsafe { *p == *p.add(2) ^ *p.add(11) }
    }

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

    pub fn parse(&mut self, buf: &[u8]) -> PacketResult {
        let mut buflen = buf.len();
        assert!(buflen > 0);

        // step 1: 确认 buf 可读位置和 未读长度
        let mut bufpos = self.pos;
        buflen = buflen - bufpos;

        if buflen == 0 {
            return PacketResult::None;
        }

        let mut current = match self.current.take() {
            Some(v) => v,
            None => RAW_POOL.pull(),
        };

        let mut current_len = current.0.len();

        // 解析消息头
        if current_len < Packet::HEAD_SIZE {
            let mut nleft = Packet::HEAD_SIZE - current_len;
            if nleft > buflen {
                nleft = buflen;
            }

            unsafe {
                std::ptr::copy(buf.as_ptr().add(bufpos), current.0.as_mut_ptr().add(current_len), nleft);

                bufpos += nleft;
                buflen -= nleft;
                current_len += nleft;
                current.0.set_len(current_len);
            }

            // 检查消息头, 如果检查失败, 直接将失败的结果返回
            if current_len == Packet::HEAD_SIZE {
                if current.id() == 0 || current.idempotent() == 0 || !current.check_hc() {
                    return PacketResult::Err(g::Err::PackHeadInvalid);
                }
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
                std::ptr::copy(buf.as_ptr().add(bufpos), current.0.as_mut_ptr().add(current_len), nleft);

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
                return PacketResult::Err(g::Err::PacketInvalid)
            }

            return if buflen == 0 { PacketResult::Last(current) } else { PacketResult::Next(current) }
        }
        
        self.current = Some(current);
        PacketResult::None
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_packet {
    use super::Packet;

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

        let mut builder = super::Builder::new();

        for i in 0..1000 {
            tracing::debug!("------------------------------ {i}");
            let (pkt, done) = match builder.parse(&mut buf) {
                super::PacketResult::Err(err) => panic!("{err}"),
                super::PacketResult::None => break,
                super::PacketResult::Next(v) => (v, false),
                super::PacketResult::Last(v) => (v, true),
            };

            tracing::debug!("-->>> idempotent: {}", pkt.idempotent());
            assert_eq!(pkt.id(), 1);
            assert_eq!(pkt.idempotent(), i + 1);
            assert_eq!(hex::encode(data), hex::encode(pkt.data()));
            assert_eq!(pkt.raw().len(), pkt.raw_len());

            if done {
                assert_eq!(i, 99);
                break;
            }
        }
            
    }
}
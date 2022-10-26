use crate::g;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use std::fmt::Display;

// ----------------------------------------------- 类型重定义 -----------------------------------------------

/// Packet 对象池元素类型
pub type LinearItem = LinearReusable<'static, Packet>;


// ----------------------------------------------- 静态对象 -----------------------------------------------

lazy_static::lazy_static! {
    /// packet 内部使用对象池.
    static ref INNER_POOL: LinearObjectPool<Packet> = LinearObjectPool::new(Packet::default, |v|{v.reset();});

    /// 构建请求包时使用
    pub static ref REQ_POOL: LinearObjectPool<Packet> = LinearObjectPool::new(Packet::default, |v|{v.reset();});

    /// 构建应答包时使用
    pub static ref RSP_POOL: LinearObjectPool<Packet> = LinearObjectPool::new(Packet::default, |v|{v.reset();});

    /// 通用Packet对象池
    pub static ref RKT_POOL: LinearObjectPool<Packet> = LinearObjectPool::new(Packet::default, |v|{v.reset();});
}

// ----------------------------------------------- Packet -----------------------------------------------

/// Packet 消息包类型
/// 
/// 用于网络消息传输.
/// 
/// 实现方式为 元组结构. Packet逻辑上分为消息头与消息体.
/// 
/// # 消息头
/// 
/// 消息头占 `12bytes`
/// 
/// `head_check_code` 消息头校验码, 计算方式: `head[2] ^ head[11]`.
/// 
/// `raw_check_code`  消息包校验码, 计算方式: `raw[2] ^ [raw.len - 1]`.
/// 
/// `id` 消息包ID, 用于路由消息类型
/// 
/// `idempotent` 幂等, 用于确认该消息是否过期.
/// 
/// `raw_len` 消息长度, 表示 校验和 (2bytes) + 消息头(10bytes) + 消息体长度.
/// 
/// # 消息体
/// 
/// `data` 消息包的主要业务内容
/// 
/// # 内存布局
/// 
/// || `head_check_code u8` ! `raw_check_code u8` ! `id u16` ! `idempotent u32` ! `raw_len u32` || `data [u8]` ||
pub struct Packet(Vec<u8>);

impl Packet {
    /// 消息头长度
    pub const HEAD_SIZE: usize = 12;

    /// 消息体最大长度
    pub const MAX_DATA_SIZE: usize = 1024 * 1024 * 2;

    /// 创建默认消息包
    /// 
    /// 通过new 创建的消息包, 在设置完所有字段值之后, 记得调用setup 方法让其有效. 详情参数[setup]方法.
    /// 
    /// # Example
    /// 
    /// ```
    /// let mut pkt = tg::nw::packet::Packet::new();
    /// pkt.set_id(1);
    /// pkt.set_idempotent(1);
    /// pkt.set_data("Hello".as_bytes());
    /// assert!(!pkt.valid());
    /// pkt.setup();
    /// assert!(pkt.valid());
    /// ```
    #[inline(always)]
    pub fn new() -> Self {
        Self(Vec::with_capacity(g::DEFAULT_BUF_SIZE))
    }

    /// 跟据字段创建消息包
    /// 
    /// 通过该函数创建的消息包, 创建后便是有效消息包, 不需要单独调用setup 方法让其有效. 但是如果在后期使用中有改变过字段值, 还是需要调用setup 使其有效. 详情参数[setup]方法.
    /// 
    /// # Example
    /// 
    /// ```
    /// let mut pkt = tg::nw::packet::Packet::with(1, 1, "Hello".as_bytes());
    /// assert!(pkt.valid());
    /// pkt.set_id(2);
    /// assert!(!pkt.valid());
    /// pkt.setup();
    /// assert!(pkt.valid());
    /// ```
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

    /// Getter: id
    #[inline(always)]
    pub fn id(&self) -> u16 {
        unsafe { *(self.0.as_ptr().add(2) as *const u16) }
    }

    /// Getter: idempotent
    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        unsafe { *(self.0.as_ptr().add(4) as *const u32) }
    }

    /// Getter: raw_len
    #[inline(always)]
    pub fn raw_len(&self) -> usize {
        unsafe { *(self.0.as_ptr().add(8) as *const u32) as usize }
    }

    /// 获取消息包的原始码流
    /// 
    /// # Example
    /// 
    /// ```
    /// let pkt = tg::nw::packet::Packet::with(1, 1, "Hello".as_bytes());
    /// println!("{}", hex::encode(pkt.raw()));
    /// ```
    #[inline(always)]
    pub fn raw(&self) -> &[u8] {
        &self.0
    }

    /// 获取消息体
    /// 
    /// # Example
    /// ```
    /// let pkt = tg::nw::packet::Packet::with(1, 1, "Hello".as_bytes());
    /// println!("{}", hex::encode(pkt.data()));
    /// ```
    #[inline(always)]
    pub fn data(&self) -> &[u8] {
        &self.0[Self::HEAD_SIZE..]
    }

    /// 判断消息包是否有效
    /// 
    /// 有效的消息包 id > 0 && idempotent > 0 && raw.len() == raw_len && head_check_code is ok && raw_check_code is ok.
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
    
    /// Setter: id
    #[inline(always)]
    pub fn set_id(&mut self, id: u16) {
        unsafe { *(self.0.as_mut_ptr().add(2) as *mut u16) = id }
    }

    /// Setter: idempotent
    #[inline(always)]
    pub fn set_idempotent(&mut self, idempotent: u32) {
        unsafe { *(self.0.as_mut_ptr().add(4) as *mut u32) = idempotent }
    }

    /// Setter: data
    /// 
    /// 设置消息体的时候同时会设置消息包的raw_len字段
    /// 
    /// # Example
    /// 
    /// ```
    /// let mut pkt = tg::nw::packet::Packet::new();
    /// assert!(pkt.raw().len() == 0);
    /// pkt.set_data("Hello".as_bytes());
    /// assert!(pkt.raw_len() == 17);
    /// ```
    #[inline(always)]
    pub fn set_data(&mut self, data: &[u8]) {
        let dlen = data.len(); 
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

    /// 使消息包变得有效.
    /// 
    /// 当调用过 [set_id], [set_idempotent] , [set_data] 这样的方法后, 消息包需要重新计算校验和字段
    /// 
    /// 而该方法的作用就是重新计算校验和, 这样消息包才能变得有效. 否则无法通过 [valid] 方法判断.
    #[inline(always)]
    pub fn setup(&mut self) {
        assert!(self.id() > 0 && self.idempotent() > 0 && self.raw_len() == self.raw().len());

        self.0[0] = self.0[2] ^ self.0[11];
        self.0[1] = self.0[2] ^ self.0[self.raw_len() - 1];
    }

    /// 设置消息包
    /// 
    /// 该方法会设置消息包所有字段, 包括校验和字段.
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

    /// 重置消息包, 使用变得无效
    pub(crate) fn reset(&mut self) {
        unsafe { self.0.set_len(0); }
    }

    /// 校验 head_check_code
    #[inline(always)]
    pub(crate) fn check_hc(&self) -> bool {
        let p = self.0.as_ptr();
        unsafe { *p == *p.add(2) ^ *p.add(11) }
    }

    /// 校验 raw_check_code
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

// ----------------------------------------------- Builder -----------------------------------------------

/// 消息包构造器
pub struct Builder {
    current: Option<LinearReusable<'static, Packet>>,
    pos: usize
}

impl Builder {
    /// 创建 构造器
    #[inline]
    pub fn new() -> Self {
        Self {
            current: None,
            pos: 0,
        }
    }

    /// 重置构造器
    #[inline(always)]
    pub fn reset(&mut self) {
        self.pos = 0;
        if let Some(v) = self.current.as_deref_mut() {
            v.reset();
        }
    }

    /// 通过buf 构建 Packet 对象
    /// 
    /// 该方法会从buf 中不断的读取数据, 直到无法构建成一个完整的Packet.
    /// 
    /// # Example
    /// 
    /// ```
    /// let mut buf = Vec::<u8>::new();
    ///
    /// let pkt = tg::nw::packet::Packet::with(1, 1, "Hello world".as_bytes());
    /// assert!(pkt.valid());
    /// buf.extend_from_slice(pkt.raw());
    ///
    /// let mut builder = tg::nw::packet::Builder::new();
    ///
    /// loop {
    ///     let option_pkt = match builder.parse(&mut buf) {
    ///         Err(err) => panic!("{err}"),
    ///         Ok(v) => v,
    ///     };
    ///
    ///     let (pkt, next) = match option_pkt {
    ///         None => break,
    ///         Some((pkt, v)) => (pkt, v),
    ///     };
    ///
    ///     if !next {
    ///         assert_eq!(pkt.idempotent(), 1);
    ///         break;
    ///     }
    /// }
    /// ```
    pub fn parse(&mut self, buf: &[u8]) -> g::Result<Option<(LinearItem, bool)>> {
        let mut buflen = buf.len();
        assert!(buflen > 0);

        // step 1: 确认 buf 可读位置和 未读长度
        let mut bufpos = self.pos;
        buflen -= bufpos;

        if buflen == 0 {
            return Ok(None);
        }

        let mut cur = match self.current.take() {
            Some(v) => v,
            None => INNER_POOL.pull(),
        };

        let mut clen = cur.0.len();

        // 解析消息头
        if clen < Packet::HEAD_SIZE {
            let mut nleft = Packet::HEAD_SIZE - clen;
            if nleft > buflen {
                nleft = buflen;
            }

            unsafe {
                std::ptr::copy_nonoverlapping(buf.as_ptr().add(bufpos), cur.0.as_mut_ptr().add(clen), nleft);

                bufpos += nleft;
                buflen -= nleft;
                clen += nleft;
                cur.0.set_len(clen);
            }

            // 检查消息头, 如果检查失败, 直接将失败的结果返回
            if clen == Packet::HEAD_SIZE && (cur.id() == 0 || cur.idempotent() == 0 || !cur.check_hc()) {
                return Err(g::Err::PackHeadInvalid);
            }
        }

        // 解析消息体
        if buflen > 0 && clen >= Packet::HEAD_SIZE {
            let rl = cur.raw_len();
            if cur.0.capacity() < rl {
                cur.0.reserve(rl);
            }

            let mut nleft = rl - clen;
            if nleft > buflen {
                nleft = buflen;
            }

            unsafe {
                std::ptr::copy_nonoverlapping(buf.as_ptr().add(bufpos), cur.0.as_mut_ptr().add(clen), nleft);

                bufpos += nleft;
                buflen -= nleft;
                clen += nleft;
                cur.0.set_len(clen);
            }
        }

        if buflen == 0 {
            self.pos = 0;
        } else {
            self.pos = bufpos;
        }

        // 检查消息体
        if clen == cur.raw_len() {
            if !cur.check_rc() {
                return Err(g::Err::PacketInvalid)
            }

            return if buflen == 0 { Ok(Some((cur, false))) } else { Ok(Some((cur, true))) }
        }
        
        self.current = Some(cur);
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
use crate::g;
use lazy_static::lazy_static;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use std::{sync::Arc, fmt::Display};
use type_layout::TypeLayout;

// ---------------------------------------------------- 对象池 ----------------------------------------------------
// 对象池使用的是低级的内存访问方式, 是以牺牲语言安全性的代价换来更好的性能
//
//
lazy_static! {
    /// 在内部使用的 package 对象池
    pub(crate) static ref PACK_POOL: LinearObjectPool<Package> = LinearObjectPool::new(Package::new, |v|v.reset());

    /// 该对象池用于向对端发送数据.
    pub static ref WBUF_POOL: LinearObjectPool<Vec<u8>> = LinearObjectPool::new(|| Vec::with_capacity(2048), |_v| {});

    /// 该对象池用于处理 请求包
    pub static ref REQ_POOL: LinearObjectPool<Package> = LinearObjectPool::new(Package::new, |v|v.reset());

    /// 该对象池用于处理 应答包
    pub static ref RSP_POOL: LinearObjectPool<Package> = LinearObjectPool::new(Package::new, |v|v.reset());
}

/// package 原始缓冲区数据
/// 
/// 该类型对象是从WBUF_POOL对象池中获取的数据, 使用完后会返还WBUF_POOL中.
/// 
/// 加 Arc的意义
/// 
/// `因为该类型的实例需要在 tokio::broadcast 管道中传递所以该类型必需实现Clone 特征
/// 而 LinearReusable<'static, BytesMut> 并没有实现 Clone特征, 所以这里需要在外围加上一层 Arc, 这样该类型数据才能在管道中间被传递.`
pub type PackBuf = Arc<LinearReusable<'static, Vec<u8>>>;

/// RspBuf 应答缓冲区
/// 
/// Option<PackBuf> 的别名.
pub type RspBuf = Option<PackBuf>;


/// Package 数据包, 用于网络传输.
/// 
/// 在多次实验后, 最终选择了原始数据类型的Package定义, 目的当然为了极致的速度.
/// 
/// package 只有两个字段组成:
/// 
/// `raw` 表示一个连续的内存空间.
/// 
/// `raw_pos` 表示 raw的有效范围, 即 `raw[0..raw_pos]` 表示有效的数据.
/// 
/// 虽然只有两个字段, 但实际上 package有着稍许复杂的逻辑设计.
/// 
/// Package 由 `消息头` 和 `消息体` 组成.
/// 
/// # 消息头
/// 
/// 消息头由 5个字段组成, 共占 12 个字节.
/// 
/// `package_id` [u16] 表示 消息ID.
/// 
/// `idempotent` [u32] 表示 幂等, 用于重复消息过滤.
/// 
/// `raw_len`    [u32] 消息长度, 包括消息体, 例如: 消息数据为 b'Hello world', 那么 raw_len 为 12(消息头长度) + 11(消息数据长度) = 23.
/// 
/// `head_code`  [u8]  用于校验 消息头 是否有效, 该值的计算方式为 `raw[0] ^ raw[9]`, 由消息头第一个字节 异或 消息头第10个字节.
/// 
/// `raw_code`   [u8]  用于校验 package是否有效, 该值的计算方式为 `raw[0] ^ [raw_pos]`, 由原始数据第一个字节 异或 原始数据最后一个字节.
/// 
/// # 内存部局
/// 
/// `| package_id: 2bytes | idempotent: 4bytes | raw_len: 4bytes | head_code: 1byte | raw_code: 1byte | data nbytes |`
/// 
/// # PS
/// 
/// 消息头中的各字段按小端序排列
#[derive(TypeLayout, Debug)]
pub struct Package {
    raw_pos: usize, // 有效位置
    raw: Vec::<u8>, // 原始内存空间
}

unsafe impl Sync for Package{}
unsafe impl Send for Package{}

impl Package {
    /// 消息头长度
    pub const HEAD_SIZE: usize = 12;
    /// 最大消息长度: 128M
    pub const MAX_DATA_SIZE: usize = 1024 * 1024 * 128;

    /// 创建空 Package
    pub fn new() -> Self {
        Self { 
            raw: {
                #[allow(clippy::uninit_vec)]
                unsafe {
                    let mut v = Vec::with_capacity(2048);
                    v.set_len(2048);
                    v
                }
            },
            raw_pos: 0,
        }
    }

    /// 跟据入参创建 有效 Package
    pub fn with_params(package_id: u16, idempotent: u32, data: &[u8]) -> Self {
        let mut res = Self::new();
        res.set(package_id, idempotent, data);
        res
    }

    /// 重置 Package
    #[inline(always)]
    pub fn reset(&mut self) {
        self.raw_pos = 0;
    }

    /// 序列化到 buf中.
    #[inline(always)]
    pub fn to_bytes(&self, buf: &mut Vec<u8>) {
        if buf.len() < self.raw_pos && buf.capacity() < self.raw_pos {
            buf.reserve(self.raw_pos);
        }

        unsafe {
            std::ptr::copy(self.raw.as_ptr() , buf.as_mut_ptr(), self.raw_pos);
            buf.set_len(self.raw_pos); 
        }
    }

    /// 从buf中反序列化到当前package对象中.
    /// 
    /// 返回值为 从buf中消费的字节长度.
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

            unsafe { std::ptr::copy(buf.as_ptr(), self.raw.as_mut_ptr().add(self.raw_pos), nleft); }
            self.raw_pos += nleft;
            buf_pos += nleft;

            // 检查 head_code
            if self.raw_pos == Self::HEAD_SIZE && (self.raw[0] ^ self.raw[9] != self.head_code() || self.package_id() == 0 || self.idempotent() == 0) {
                return Err(g::Err::PackHeadInvalid);
            }
        }

        // 处理消息体
        if self.raw_pos >= Self::HEAD_SIZE {
            let raw_len = self.raw_len();
            if buf_len > buf_pos /* buf 中还有多余的数据未读取 */ && self.raw_pos < raw_len /* 消息体没有读满 */ {
                if self.raw.capacity() < raw_len {
                    #[allow(clippy::uninit_vec)]
                    unsafe {
                        self.raw.reserve(raw_len);
                        self.raw.set_len(raw_len); 
                    }
                }

                let mut nleft = raw_len - self.raw_pos;
                let n = buf_len - buf_pos;
                if nleft > n {
                    nleft = n;
                }
    
                unsafe { 
                    std::ptr::copy(buf.as_ptr().add(buf_pos), self.raw.as_mut_ptr().add(self.raw_pos), nleft); 
                }
                self.raw_pos += nleft;
                buf_pos += nleft;

                // 检查 raw_code
                if self.raw_pos == raw_len && self.raw[0] ^ self.raw[self.raw_pos - 1] != self.raw_code() {
                    return Err(g::Err::PackageInvalid);
                }
            }
        }

        Ok(buf_pos)
    }

    /// setup 会将package 包的 head_code 和 raw_code 打上, 并设置raw_pos.
    /// 
    /// 如果创建的是一个空 Package, package 在设置了各个字段之后, 是不会打上 head_code和 raw_code, 并且 raw_pos也不会有改变.
    /// 这将导致该 Package 为无效实例 `this.valid() == false`. 所以在设置完该实例的所有字段之后要调用 setup 函数, 才能让该实例有效.
    /// 
    /// # Example
    /// 
    /// ```
    /// use tg::nw::pack::Package;
    ///
    /// let mut p = Package::new();
    /// p.set_package_id(1);
    /// p.set_idempotent(2);
    /// p.set_data("Hello world".as_bytes());
    /// 
    /// assert!(!p.valid());
    /// p.setup();
    /// assert!(p.valid());
    /// ```
    #[inline(always)]
    pub fn setup(&mut self) {
        assert!(self.package_id() > 0 && self.idempotent() > 0);
        self.raw_pos = self.raw_len();
        self.raw[10] = self.raw[0] ^ self.raw[9];
        self.raw[11] = self.raw[0] ^ self.raw[self.raw_pos - 1];
    }

    /// valid 判断 package是否有效
    #[inline(always)]
    pub fn valid(&self) -> bool {
        self.package_id() > 0 && self.idempotent() > 0 && self.raw_pos == self.raw_len()
    }

    /// 设置所有字段
    #[inline]
    pub fn set(&mut self, package_id: u16, idempotent: u32, data: &[u8]) {
        assert!(package_id > 0 && idempotent > 0);

        let data_len = data.len();
        assert!(data_len < Self::MAX_DATA_SIZE);

        self.raw_pos = data_len + Self::HEAD_SIZE;

        if self.raw_pos > self.raw.capacity() {
            #[allow(clippy::uninit_vec)]
            unsafe {
                self.raw.reserve(self.raw_pos);
                self.raw.set_len(self.raw_pos); 
            }
        }

        unsafe {
            let p = self.raw.as_mut_ptr();
            std::ptr::copy(&package_id as *const u16 as *const u8, p, 2);
            std::ptr::copy(&idempotent as *const u32 as *const u8, p.add(2), 4);
            std::ptr::copy(&(self.raw_pos as u32) as *const u32 as *const u8, p.add(6), 4);
            std::ptr::copy(data.as_ptr(), p.add(12), data_len);
        }
        
        self.raw[10] = self.raw[0] ^ self.raw[9];
        self.raw[11] = self.raw[0] ^ self.raw[self.raw_pos - 1];
    }

    /// 设置 package_id
    #[inline(always)]
    pub fn set_package_id(&mut self, package_id: u16) {
        assert!(package_id > 0);
        unsafe { *(self.raw.as_mut_ptr() as *mut u16) = package_id; }
    }

    /// 获取 package_id
    #[inline(always)]
    pub fn package_id(&self) -> u16 {
        unsafe {*(self.raw.as_ptr() as *const u8 as *const u16)}
    }

    /// 设置幂等
    #[inline(always)]
    pub fn set_idempotent(&mut self, idempotent: u32) {
        assert!(idempotent > 0);
        unsafe { *(self.raw.as_mut_ptr().add(2) as *mut u32) = idempotent; }
    }

    /// 获取幂等
    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        unsafe {*(self.raw.as_ptr().add(2) as *const u8 as *const u32)}
    }

    /// 获取 消息头校验码
    #[inline(always)]
    pub fn head_code(&self) -> u8 {
        self.raw[10]
    }

    /// 获取 消息包校验码
    #[inline(always)]
    pub fn raw_code(&self) -> u8 {
        self.raw[11]
    }

    /// 获取消息包长度
    #[inline(always)]
    pub fn raw_len(&self) -> usize {
        unsafe {*(self.raw.as_ptr().add(6) as *const u8 as *const u32) as usize}
    }

    /// 设置消息体
    #[inline]
    pub fn set_data(&mut self, data: &[u8]) {
        let data_len = data.len();
        assert!(data_len < Self::MAX_DATA_SIZE);

        let raw_len = data_len + Self::HEAD_SIZE;

        if self.raw.capacity() < raw_len {
            self.raw.reserve(raw_len);
        }

        unsafe { 
            *(self.raw.as_mut_ptr().add(6) as *mut u32) = raw_len as u32;
            std::ptr::copy(data.as_ptr(), self.raw.as_mut_ptr().add(12), data_len);
            self.raw.set_len(raw_len);
        }
    }

    /// 获取消息体
    #[inline(always)]
    pub fn data(&self) -> &[u8] {
        &self.raw[Self::HEAD_SIZE..self.raw_len()]
    }

    /// 获取原始数据
    #[inline(always)]
    pub fn raw(&self) -> &[u8] {
        &self.raw[..self.raw_len()]
    }
}

impl Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[package_id: {}; idempotent: {}; raw_len: {}; data: {}]", self.package_id(), self.idempotent(), self.raw_len(), hex::encode(self.data()))
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
        p1.setup();

        assert_eq!(1, p1.package_id());
        assert_eq!(2, p1.idempotent());
        assert_eq!(Package::HEAD_SIZE + data.len(), p1.raw_len());
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), data);

        let mut buf = crate::nw::pack::WBUF_POOL.pull();
        p1.to_bytes(&mut buf);

        assert_eq!(p1.head_code(), p1.raw[0] ^ p1.raw[9]);
        assert_eq!(p1.raw_code(), p1.raw[0] ^ p1.raw[Package::HEAD_SIZE + 10]);

        let mut p2 = Package::new();
        p2.from_bytes(&buf).unwrap();

        assert_eq!(p2.package_id(), p1.package_id());
        assert_eq!(p2.idempotent(), p1.idempotent());
        assert_eq!(p2.raw_len(), p1.raw_len());
        assert_eq!(std::str::from_utf8(p1.data()).unwrap(), std::str::from_utf8(p2.data()).unwrap());
    }
}
// ---------------------------
// tg::nw::pck
//     网络包定义
//
// @作者: iegad
//
// @时间: 2022-08-10
// ---------------------------

use crate::g;
use core::slice::from_raw_parts;
use lazy_static::lazy_static;
use lockfree_object_pool::{LinearObjectPool, LinearReusable};
use std::sync::Arc;

/// 消息头
///
/// 消息头为定长 8 字节
///
/// # 内存布局
/// | [pid] 2字节 | [idempotent] 4字节 | [raw_len] 4字节 |
///
/// ## pid
///
/// 消息包ID, 用于区分消息行为.
///
/// ## idempotent 幂等
///
/// 幂等, 用于检测包是否为重复包.
///
/// ## raw_len
///
/// 消息长度, 消息原始长度 消息头[10 bytes] + 消息体[N bytes].
pub struct Package {
    raw: Vec<u8>,   //  原数据
    raw_pos: usize, // 接收位置
}

pub type PackageItem<'a> = LinearReusable<'a, Package>;
type PackagePool = LinearObjectPool<Package>;

lazy_static! {
    /// Package 对象池
    ///
    /// # Example
    ///
    /// ```
    /// let mut p1 = tg::nw::pack::PACK_POOL.pull();
    /// p1.set_pid(10);
    /// p1.set_idempotent(1000);
    /// p1.set_data("hello world".as_bytes());
    /// assert_eq!(p1.pid(), 10);
    /// assert_eq!(p1.idempotent(), 1000);
    /// assert_eq!("hello world", core::str::from_utf8(p1.data()).unwrap());
    /// ```
    pub static ref PACK_POOL: Arc<PackagePool> = {
        let v = Arc::new(PackagePool::new(
            || Package::new(),
            |v| { v.raw_pos = 0; },
        ));
        v
    };
}

impl Package {
    /// Package 原始数据初始化长度.
    /// 随着后期的调用, 原始数据长度为发生变化, 但决不会小于 [Pack::RAW_SIZE].
    pub const RAW_SIZE: usize = 4096;
    pub const HEAD_SIZE: usize = 10;

    /// 创建一个空包对象
    pub fn new() -> Package {
        Package {
            raw: vec![0u8; Self::RAW_SIZE],
            raw_pos: 0,
        }
    }

    /// 创建一个有初始化值的包对象
    pub fn with_params(pid: u16, idempotent: u32, data: &[u8]) -> Package {
        let raw_len = data.len() + 10;
        let mut raw = vec![0u8; raw_len];

        // 将 pid 写入原始码流
        raw[..2].copy_from_slice(unsafe { from_raw_parts(&pid as *const u16 as *const u8, 2) });

        // 将 idempotent 写入原始码流
        raw[2..6]
            .copy_from_slice(unsafe { from_raw_parts(&idempotent as *const u32 as *const u8, 4) });

        // 将 raw_len 写入原始码流
        raw[6..10].copy_from_slice(unsafe {
            from_raw_parts(&(raw_len as u32) as *const u32 as *const u8, 4)
        });

        // 将数据写入原始码流
        raw[10..raw_len].copy_from_slice(data);

        Package { raw, raw_pos: 0 }
    }

    /// 获取码流的可写区域
    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        &mut self.raw[self.raw_pos..]
    }

    /// 将 self.raw[..n] 转换为 package.
    ///
    /// 成功转换为一个完整的包返回 true.
    ///
    /// 未成功转换为一个完整的包(后续还需要追加码流才能成功完整的Package) 返回 false.
    ///
    /// 无效的码流, 返回相应错误.
    pub fn parse(&mut self, n: usize) -> g::Result<bool> {
        if self.raw_pos == 0 && n < Self::HEAD_SIZE {
            return Err(g::Err::PackHeadInvalid("raw size is not long enough"));
        }

        let raw_len = self.raw_len();

        // 当raw的长度不够时, 调整 raw大小
        if raw_len > self.raw.capacity() {
            self.raw.resize(raw_len, 0);
        }

        if self.pid() == 0 {
            return Err(g::Err::PackHeadInvalid("pid is invalid"));
        }

        if self.idempotent() == 0 {
            return Err(g::Err::PackHeadInvalid("idempotent is invalid"));
        }

        self.raw_pos += n;

        let res = raw_len == self.raw_pos;

        if res {
            // 当前数据已经是完整的 Package时, 重置可写位置.
            self.raw_pos = 0;
        }

        Ok(res)
    }

    /// 返回源始码流
    pub fn to_bytes(&self) -> &[u8] {
        &self.raw[..self.raw_len()]
    }

    /// 返回 pid
    pub fn pid(&self) -> u16 {
        unsafe { *(self.raw.as_ptr() as *const u16) }
    }

    /// 设置 pid
    pub fn set_pid(&mut self, pid: u16) {
        self.raw[..2]
            .copy_from_slice(unsafe { from_raw_parts(&pid as *const u16 as *const u8, 2) });
    }

    /// 返回 幂等
    pub fn idempotent(&self) -> u32 {
        unsafe { *(self.raw.as_ptr().add(2) as *const u32) }
    }

    /// 设置 幂等
    pub fn set_idempotent(&mut self, idempotent: u32) {
        self.raw[2..6]
            .copy_from_slice(unsafe { from_raw_parts(&idempotent as *const u32 as *const u8, 4) });
    }

    /// 返回原始码流长度
    pub fn raw_len(&self) -> usize {
        unsafe { *(self.raw.as_ptr().add(6) as *const u32) as usize }
    }

    /// 获取消息体数据
    pub fn data(&self) -> &[u8] {
        &self.raw[10..self.raw_len()]
    }

    /// 设置消息体数据
    pub fn set_data(&mut self, data: &[u8]) {
        let raw_len = data.len() + 10;

        if raw_len > self.raw.capacity() {
            self.raw.resize(raw_len, 0);
        }

        self.raw[6..10].copy_from_slice(unsafe {
            from_raw_parts(&(raw_len as u32) as *const u32 as *const u8, 4)
        });

        self.raw[10..raw_len].copy_from_slice(data);
    }
}

#[cfg(test)]
mod pcomp_tester {
    use super::Package;

    #[test]
    fn test_package() {
        let s = "hello world";
        let p1 = Package::with_params(0x01, 0x02, s.as_bytes());
        assert_eq!(p1.pid(), 0x01);
        assert_eq!(p1.idempotent(), 0x02);
        assert_eq!(p1.raw_len(), 21);
        assert_eq!(s, core::str::from_utf8(p1.data()).unwrap());

        let buf = p1.to_bytes();
        let mut p2 = Package::new();
        p2.as_mut_bytes()[..buf.len()].copy_from_slice(buf);

        assert_eq!(p1.pid(), p2.pid());
        assert_eq!(p1.idempotent(), p2.idempotent());
        assert_eq!(p1.raw_len(), p2.raw_len());
        assert_eq!(
            core::str::from_utf8(p1.data()).unwrap(),
            core::str::from_utf8(p2.data()).unwrap()
        );
    }
}

// ---------------------------
// tg::nw::pck
//     网络包定义
//
// @作者: iegad
//
// @时间: 2022-08-10
// ---------------------------

use crate::g;

/// 消息头
///
/// 消息头为定长 18 字节
///
/// # 内存布局
///
/// | [pid] 2字节 | [router_id] 0 ~ 8字节 | [data_len] 0 ~ 4字节 |
#[derive(Debug)]
pub struct Head {
    // ---------------------------
    // 数据字段定义
    pid: u16,        // PackageID
    idempotent: u32, // 幂等
    router_id: u64,  // 路由ID
    data_len: usize, // 消息体长度
}

impl Head {
    pub const SIZE: usize = 18;

    /// 创建默认的 tg::pack::Head 实例.
    pub fn new() -> Head {
        Head {
            pid: 0,
            idempotent: 0,
            router_id: 0,
            data_len: 0,
        }
    }

    /// 通过字段构建 tg::pack::Head 实例
    ///
    /// @pid:        包ID
    ///
    /// @idempotent: 幂等
    ///
    /// @user_id:    用户ID
    ///
    /// @data_len:   消息体长度
    ///
    /// # PS
    ///
    /// pid 和 idempotent 必需大于0
    pub fn with_params(pid: u16, idempotent: u32, router_id: u64, data_len: usize) -> Head {
        assert!(pid > 0 && idempotent > 0);

        Head {
            pid,
            idempotent,
            router_id,
            data_len,
        }
    }

    /// 消息头序列化到指定的缓冲区中
    pub fn to_bytes(&self, buf: &mut [u8]) {
        use core::slice::from_raw_parts;

        assert!(buf.len() >= Self::SIZE);

        buf[..2]
            .copy_from_slice(unsafe { from_raw_parts(&self.pid as *const u16 as *const u8, 2) });
        buf[2..6].copy_from_slice(unsafe {
            from_raw_parts(&self.idempotent as *const u32 as *const u8, 4)
        });
        buf[6..14].copy_from_slice(unsafe {
            from_raw_parts(&self.router_id as *const u64 as *const u8, 8)
        });
        buf[14..18].copy_from_slice(unsafe {
            from_raw_parts(&(self.data_len as u32) as *const u32 as *const u8, 4)
        });
    }

    /// 通过缓冲区的数据构建消息头
    pub fn parse(&mut self, buf: &[u8]) -> g::Result<()> {
        let buf_len = buf.len();
        assert!(buf_len >= Self::SIZE);

        let ptr = buf.as_ptr();
        self.pid = unsafe { *(ptr as *const u16) };
        self.idempotent = unsafe { *(ptr.add(2) as *const u32) };
        self.router_id = unsafe { *(ptr.add(6) as *const u64) };
        self.data_len = unsafe { *(ptr.add(14) as *const u32) as usize };

        Ok(())
    }
}

impl Head {
    /// 设置 消息ID
    #[inline(always)]
    pub fn set_pid(&mut self, pid: u16) {
        assert!(pid > 0);
        self.pid = pid;
    }

    #[inline(always)]
    pub fn set_idempotent(&mut self, idempotent: u32) {
        assert!(idempotent > 0);
        self.idempotent = idempotent;
    }

    /// 设置 用户ID
    #[inline(always)]
    pub fn set_router_id(&mut self, router_id: u64) {
        self.router_id = router_id;
    }

    /// 设置 消息体长度
    #[inline(always)]
    pub fn set_data_len(&mut self, data_len: usize) {
        self.data_len = data_len;
    }
}

#[derive(Debug)]
pub struct Package {
    head: Head,
    data: Vec<u8>,
    data_pos: usize,
}

impl Package {
    pub fn new() -> Package {
        Package {
            head: Head::new(),
            data: Vec::new(),
            data_pos: 0,
        }
    }

    pub fn with_params(pid: u16, idempotent: u32, router_id: u64, data: &[u8]) -> Package {
        Package {
            head: Head {
                pid,
                idempotent,
                router_id,
                data_len: data.len(),
            },
            data: data.to_vec(),
            data_pos: 0,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0u8; Head::SIZE + self.head.data_len];
        self.head.to_bytes(&mut buf);
        buf[Head::SIZE..].copy_from_slice(&self.data);

        buf
    }

    #[inline(always)]
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    #[inline(always)]
    pub fn pid(&self) -> u16 {
        self.head.pid
    }

    #[inline(always)]
    pub fn idempotent(&self) -> u32 {
        self.head.idempotent
    }

    #[inline(always)]
    pub fn router_id(&self) -> u64 {
        self.head.router_id
    }

    #[inline(always)]
    pub fn data_len(&self) -> usize {
        self.head.data_len
    }

    pub fn set(&mut self, pid: u16, idempotent: u32, router_id: u64, data: &[u8]) {
        let data_len = data.len();

        self.head.set_pid(pid);
        self.head.set_idempotent(idempotent);
        self.head.set_router_id(router_id);
        self.head.set_data_len(data_len);
        if self.data.capacity() < data_len {
            self.data.resize(data_len, 0);
        }

        self.data[..data_len].copy_from_slice(&data);
    }

    pub fn parse(&mut self, buf: &[u8]) -> g::Result<bool> {
        let mut nsize = buf.len();

        if self.data_pos == 0 {
            self.head.parse(&buf)?;

            if self.data.capacity() < self.head.data_len {
                self.data.resize(self.head.data_len, 0);
            }

            nsize -= Head::SIZE;
            self.data[self.data_pos..nsize].copy_from_slice(&buf[Head::SIZE..]);
        } else {
            self.data[self.data_pos..self.data_pos + nsize].copy_from_slice(&buf);
        }

        self.data_pos += nsize;

        Ok(self.data_pos > 0 && self.head.data_len == self.data_pos)
    }

    pub fn clear(&mut self) {
        self.data_pos = 0;
    }
}

#[cfg(test)]
mod pcomp_tester {
    use crate::utils;

    use super::{Head, Package};

    #[test]
    fn test_head() {
        // --------------------------------------
        let mut h1 = Head::new();
        h1.set_pid(1);
        h1.set_idempotent(2);
        h1.set_router_id(3);
        h1.set_data_len(4);

        let mut buf = vec![0u8; Head::SIZE];

        h1.to_bytes(&mut buf);

        let mut h2 = Head::new();
        h2.parse(&buf).unwrap();

        assert_eq!(h1.pid, h2.pid);
        assert_eq!(h1.idempotent, h2.idempotent);
        assert_eq!(h1.router_id, h2.router_id);
        assert_eq!(h1.data_len, h2.data_len);
    }

    #[test]
    fn test_pack() {
        let data = "hello world".as_bytes();

        let beg = utils::now_unix_mills();

        for _ in 0..1_000_000 {
            let mut in_pack = Package::new();
            let mut out_pack = Package::new();

            in_pack.set(0x100, 0x1000, 0, data);
            let buf = in_pack.to_bytes();
            assert_eq!(buf.len(), 29);

            let res = out_pack.parse(&buf).unwrap();
            assert!(res);

            assert_eq!(out_pack.pid(), in_pack.pid());
            assert_eq!(out_pack.idempotent(), in_pack.idempotent());
            assert_eq!(out_pack.router_id(), in_pack.router_id());
            assert_eq!(out_pack.data_len(), in_pack.data_len());
            assert_eq!(out_pack.data(), data);

            out_pack.clear();
        }

        println!("总耗时??: {} ms", utils::now_unix_mills() - beg);
    }
}

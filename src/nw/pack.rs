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
/// 消息头为定长 8 字节
///
/// # 内存布局
pub struct Package {
    raw: Vec<u8>,
    raw_pos: usize,
}

impl Package {
    pub fn new() -> Package {
        Package {
            raw: vec![0u8; 4096],
            raw_pos: 0,
        }
    }

    pub fn with_params(pid: u16, idempotent: u32, data: &[u8]) -> Package {
        use core::slice::from_raw_parts;

        let mut raw = vec![0u8; data.len() + 10];

        raw[..2].copy_from_slice(unsafe { from_raw_parts(&pid as *const u16 as *const u8, 2) });
        raw[2..6]
            .copy_from_slice(unsafe { from_raw_parts(&idempotent as *const u32 as *const u8, 4) });
        raw[6..10].copy_from_slice(unsafe {
            from_raw_parts(&(data.len() as u32) as *const u32 as *const u8, 4)
        });

        raw[10..data.len() + 10].copy_from_slice(data);

        Package { raw, raw_pos: 0 }
    }

    pub fn as_bytes(&mut self) -> &mut [u8] {
        &mut self.raw[self.raw_pos..]
    }

    pub fn parse(&mut self, n: usize) -> g::Result<bool> {
        let raw_len = self.data_len() + 10;
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

        Ok(raw_len == self.raw_pos)
    }

    pub fn clear(&mut self) {
        self.raw_pos = 0;
    }

    pub fn to_bytes(&self) -> &[u8] {
        &self.raw
    }

    pub fn pid(&self) -> u16 {
        unsafe { *(self.raw.as_ptr() as *const u16) }
    }

    pub fn idempotent(&self) -> u32 {
        unsafe { *(self.raw.as_ptr().add(2) as *const u32) }
    }

    pub fn data_len(&self) -> usize {
        unsafe { *(self.raw.as_ptr().add(6) as *const u32) as usize }
    }

    pub fn data(&self) -> &[u8] {
        &self.raw[10..self.data_len() + 10]
    }
}

#[cfg(test)]
mod pcomp_tester {
    use crate::utils;

    use super::Package;

    #[test]
    fn test_package() {
        let beg = utils::now_unix_nanos();

        for _ in 0..1000000 {
            let p1 = Package::with_params(0x01, 0x02, "hello world".as_bytes());
            assert_eq!(p1.pid(), 0x01);
            assert_eq!(p1.idempotent(), 0x02);
            assert_eq!(p1.data_len(), 11);
            assert_eq!("hello world", core::str::from_utf8(p1.data()).unwrap());

            let buf = p1.to_bytes();
            let mut p2 = Package::new();
            p2.as_bytes()[..buf.len()].copy_from_slice(buf);

            assert_eq!(p1.pid(), p2.pid());
            assert_eq!(p1.idempotent(), p2.idempotent());
            assert_eq!(p1.data_len(), p2.data_len());
            assert_eq!(
                core::str::from_utf8(p1.data()).unwrap(),
                core::str::from_utf8(p2.data()).unwrap()
            );
        }

        println!("总耗时: {} nano", utils::now_unix_nanos() - beg);
    }
}

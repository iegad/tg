// ---------------------------
// tg::nw::消息头压缩协议
//     网络包定义
//
// @作者: iegad
//
// @时间: 2022-08-10
// ---------------------------

use crate::g;

/// 消息头
///
/// 消息头为变长消息头, 在解析时, 需要跟据 head.flag 来确定消息头的长度. 消息头长度: 2 ~ 15字节
///
/// # 内存布局
///
/// | [flag] 1字节 | [pid] 1 ~ 2字节 | [router_id] 0 ~ 8字节 | [data_len] 0 ~ 4字节 |
#[derive(Debug)]
pub struct Head {
    // ---------------------------
    // 数据字段定义
    flag: u8,        // 标识字段
    pid: u16,        // PackageID
    idempotent: u32, // 幂等
    router_id: u64,  // 路由ID
    data_len: usize, // 消息体长度

    // ---------------------------
    // 原始数据定义
    size: usize, // 消息头长度
}

impl Head {
    const FLAG_PID_16: u8 = 0b10000000; // 表示 head.pid 占用 2字节
    const FLAG_IDEMPOTENT_32: u8 = 0b01000000; // 表示 head.idempotent 占用 4字节
    const FLAG_IDEMPOTENT_16: u8 = 0b00100000; // 表示 head.idempotent 占用 2字节
    const FLAG_ROUTER_ID_64: u8 = 0b00011100; // 表示 router_id 占用 8字节
    const FLAG_ROUTER_ID_32: u8 = 0b00001100; // 表示 router_id 占用 4字节
    const FLAG_ROUTER_ID_16: u8 = 0b00001000; // 表示 router_id 占用 2字节
    const FLAG_ROUTER_ID_8: u8 = 0b00000100; // 表示 router_id 占用 1字节
    const FLAG_DATA_LEN_32: u8 = 0b00000011; // 表示 data_len 占用 4字节
    const FLAG_DATA_LEN_16: u8 = 0b00000010; // 表示 data_len 占用 2字节
    const FLAG_DATA_LEN_8: u8 = 0b00000001; // 表示 data_len 占用 1字节

    const MIN_SIZE: usize = 3; // 消息头最小长度

    /// 创建默认的 tg::pack::Head 实例.
    ///
    /// # PS
    ///
    /// 默认的实例, 是否法正常使用; 且 默认的实例调用 size方法时, 会显示 长度为3.
    pub fn new() -> Head {
        Head {
            flag: 0,
            pid: 0,
            idempotent: 0,
            router_id: 0,
            data_len: 0,
            size: Self::MIN_SIZE,
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
        assert!(pid > 0);

        let mut flag = 0;

        // 消息头长度
        let mut size: usize = Self::MIN_SIZE;

        // 当pid = [0x01,  0xFF]   时, pid 占用 1字节(8bit),   FLAG_PID = 0, 同时也是默认的行为.
        // 当pid = [0x100, 0xFFFF] 时, pid 占用 2字节 (16bit), FLAG_PID = 1.
        if pid > 0xFF {
            flag |= Self::FLAG_PID_16;
            size += 1;
        }

        if idempotent > 0xFFFF {
            flag |= Self::FLAG_IDEMPOTENT_32;
            size += 3;
        } else if idempotent > 0xFF {
            flag |= Self::FLAG_IDEMPOTENT_16;
            size += 1;
        }

        // 当 router_id = [0x100000000, 0xFFFFFFFFFFFFFFFF] 时, router_id 占用 8字节(64bit), FLAG_USER_ID = 7.
        // 当 router_id = [0x1000,      0xFFFFFFFF]         时, router_id 占用 4字节(32bit), FLAG_USER_ID = 3.
        // 当 router_id = [0x100,       0xFFFF]             时, router_id 占用 2字节(16bit), FLAG_USER_ID = 2.
        // 当 router_id = [0x1,         0xFF]               时, router_id 占用 1字节(8bit),  FLAG_USER_ID = 1.
        // 当 router_id = 0 时, user_id不占用任何空间, 不需要解析该字段, 同时也是默认的行为.
        if router_id > 0xFFFFFFFF {
            flag |= Self::FLAG_ROUTER_ID_64;
            size += 8;
        } else if router_id > 0xFFFF {
            flag |= Self::FLAG_ROUTER_ID_32;
            size += 4;
        } else if router_id > 0xFF {
            flag |= Self::FLAG_ROUTER_ID_16;
            size += 2;
        } else if router_id > 0 {
            flag |= Self::FLAG_ROUTER_ID_8;
            size += 1;
        }

        // 当 data_len = [0x1000, 0xFFFFFFFF] 时, data_len 占用 4字节(32bit), FLAG_DATA_LEN = 3.
        // 当 data_len = [0x100,  0xFFFF]     时, data_len 占用 2字节(16bit), FLAG_DATA_LEN = 2.
        // 当 data_len = [4,      0xFF]       时, data_len 占用 1字节(8bit),  FLAG_DATA_LEN = 1. 同时也是默认行为.
        if data_len > 0xFFFF {
            flag |= Self::FLAG_DATA_LEN_32;
            size += 4;
        } else if data_len > 0xFF {
            flag |= Self::FLAG_DATA_LEN_16;
            size += 2;
        } else if data_len > 0 {
            flag |= Self::FLAG_DATA_LEN_8;
            size += 1;
        }

        Head {
            flag,
            pid,
            idempotent,
            router_id,
            data_len,
            size,
        }
    }

    /// 消息头序列化到指定的缓冲区中
    pub fn to_bytes(&self, buf: &mut [u8]) {
        assert!(buf.len() == self.size);

        let flag_pid = self.flag_pid();
        let flag_idempotent = self.flag_idempotent();
        let flag_router_id = self.flag_router_id();
        let flag_data_len = self.flag_data_len();

        let mut pos = 0;

        unsafe {
            // serialize flag
            buf[pos] = self.flag;
            pos += 1;

            // serialize pid
            match flag_pid {
                2 => {
                    buf[pos..pos + 2].copy_from_slice(core::slice::from_raw_parts(
                        &self.pid as *const u16 as *const u8,
                        2,
                    ));
                    pos += 2;
                }

                1 => {
                    buf[pos..pos + 1].copy_from_slice(core::slice::from_raw_parts(
                        &(self.pid as u8) as *const u8,
                        1,
                    ));
                    pos += 1;
                }

                _ => {}
            }

            match flag_idempotent {
                4 => {
                    buf[pos..pos + 4].copy_from_slice(core::slice::from_raw_parts(
                        &self.idempotent as *const u32 as *const u8,
                        4,
                    ));
                    pos += 4;
                }

                2 => {
                    buf[pos..pos + 2].copy_from_slice(core::slice::from_raw_parts(
                        &(self.idempotent as u16) as *const u16 as *const u8,
                        2,
                    ));
                    pos += 2;
                }

                1 => {
                    buf[pos..pos + 1].copy_from_slice(core::slice::from_raw_parts(
                        &(self.idempotent as u8) as *const u8,
                        1,
                    ));
                    pos += 1;
                }

                _ => {}
            }

            // serialize router_id
            match flag_router_id {
                8 => {
                    buf[pos..pos + 8].copy_from_slice(core::slice::from_raw_parts(
                        &self.router_id as *const u64 as *const u8,
                        8,
                    ));
                    pos += 8;
                }

                4 => {
                    buf[pos..pos + 4].copy_from_slice(core::slice::from_raw_parts(
                        &(self.router_id as u32) as *const u32 as *const u8,
                        4,
                    ));
                    pos += 4;
                }

                2 => {
                    buf[pos..pos + 2].copy_from_slice(core::slice::from_raw_parts(
                        &(self.router_id as u16) as *const u16 as *const u8,
                        2,
                    ));
                    pos += 2;
                }

                1 => {
                    buf[pos..pos + 1].copy_from_slice(core::slice::from_raw_parts(
                        &(self.router_id as u8) as *const u8,
                        1,
                    ));
                    pos += 1;
                }

                _ => {}
            };

            // serialize data_len
            match flag_data_len {
                4 => {
                    buf[pos..pos + 1].copy_from_slice(core::slice::from_raw_parts(
                        &(self.data_len as u32) as *const u32 as *const u8,
                        4,
                    ));
                    pos += 4;
                }

                2 => {
                    buf[pos..pos + 2].copy_from_slice(core::slice::from_raw_parts(
                        &(self.data_len as u16) as *const u16 as *const u8,
                        2,
                    ));
                    pos += 2;
                }

                1 => {
                    buf[pos..pos + 1].copy_from_slice(core::slice::from_raw_parts(
                        &(self.data_len as u8) as *const u8,
                        1,
                    ));
                    pos += 1;
                }

                _ => {}
            }
        }

        assert_eq!(pos, self.size);
    }

    /// 通过缓冲区的数据构建消息头
    pub fn parse(&mut self, buf: &[u8]) -> g::Result<()> {
        let buf_len = buf.len();
        if buf_len < Self::MIN_SIZE {
            return Err(g::Err::PackHeadInvalid("buf is not long enough"));
        }

        let ptr = buf.as_ptr();
        self.size = Self::MIN_SIZE;
        self.flag = unsafe { *ptr };

        let flag_pid = self.flag & 0b10000000;
        let flag_idempotent = self.flag & 0b01100000;
        let flag_router_id = self.flag & 0b00011100;
        let flag_data_len = self.flag & 0b00000011;

        if buf_len < self.size {
            return Err(g::Err::PackHeadInvalid("buf is not long enough"));
        }

        let mut offset = 1;
        match flag_pid {
            Self::FLAG_PID_16 => {
                self.pid = unsafe { *(ptr.add(offset) as *const u16) };
                offset += 2;
                self.size += 1;
            }

            _ => {
                self.pid = unsafe { *(ptr.add(offset)) as u16 };
                offset += 1;
            }
        };

        match flag_idempotent {
            Self::FLAG_IDEMPOTENT_32 => {
                self.idempotent = unsafe { *(ptr.add(offset) as *const u32) };
                offset += 4;
                self.size += 3;
            }

            Self::FLAG_IDEMPOTENT_16 => {
                self.idempotent = unsafe { *(ptr.add(offset) as *const u16) as u32 };
                offset += 2;
                self.size += 1;
            }

            _ => {
                self.idempotent = unsafe { *(ptr.add(offset)) as u32 };
                offset += 1;
            }
        }

        match flag_router_id {
            Self::FLAG_ROUTER_ID_64 => {
                self.router_id = unsafe { *(ptr.add(offset) as *const u64) };
                offset += 8;
                self.size += 8;
            }
            Self::FLAG_ROUTER_ID_32 => {
                self.router_id = unsafe { *(ptr.add(offset) as *const u32) as u64 };
                offset += 4;
                self.size += 4;
            }
            Self::FLAG_ROUTER_ID_16 => {
                self.router_id = unsafe { *(ptr.add(offset) as *const u16) as u64 };
                offset += 2;
                self.size += 2;
            }
            Self::FLAG_ROUTER_ID_8 => {
                self.router_id = unsafe { *(ptr.add(offset)) as u64 };
                offset += 1;
                self.size += 1;
            }
            _ => {}
        };

        match flag_data_len {
            Self::FLAG_DATA_LEN_32 => {
                self.data_len = unsafe { *(ptr.add(offset) as *const u32) as usize };
                offset += 4;
                self.size += 4;
            }
            Self::FLAG_DATA_LEN_16 => {
                self.data_len = unsafe { *(ptr.add(offset) as *const u16) as usize };
                offset += 2;
                self.size += 2;
            }
            Self::FLAG_DATA_LEN_8 => {
                self.data_len = unsafe { *(ptr.add(offset)) as usize };
                offset += 1;
                self.size += 1;
            }
            _ => {}
        };

        assert_eq!(offset, self.size);

        Ok(())
    }
}

impl Head {
    /// # 获取 PID 标识(FLAG)
    ///
    /// # Returns
    ///
    /// 返回 pid 所占字节数
    pub fn flag_pid(&self) -> u8 {
        if (self.flag & 0b10000000) == 0 {
            1
        } else {
            2
        }
    }

    pub fn flag_idempotent(&self) -> u8 {
        match self.flag & 0b01100000 {
            0b00000000 => 1,
            0b00100000 => 2,
            0b01000000 => 4,
            _ => panic!(""),
        }
    }

    /// # 获取 user_id 标识(FLAG)
    ///
    /// # Returns
    ///
    /// 返回 user_id 所占用字节数
    pub fn flag_router_id(&self) -> u8 {
        match self.flag & 0b00011100 {
            0b000_000_00 => 0,
            0b000_001_00 => 1,
            0b000_010_00 => 2,
            0b000_011_00 => 4,
            0b000_111_00 => 8,
            _ => panic!(""),
        }
    }

    /// 获取 data_len 标识(FLAG)
    ///
    /// # Returns
    ///
    /// 返回 data_len 所占用字节数
    pub fn flag_data_len(&self) -> u8 {
        match self.flag & 0b00000011 {
            0b00000000 => 0,
            0b00000001 => 1,
            0b00000010 => 2,
            0b00000011 => 4,
            _ => panic!(""),
        }
    }

    /// 设置 消息ID
    pub fn set_pid(&mut self, pid: u16) {
        assert!(pid > 0);

        if pid > 0xFF {
            if self.pid < 0xFF {
                self.flag = self.flag & 0b_0111_1111 | Self::FLAG_PID_16;
                self.size += 1;
            }
        } else {
            if self.pid > 0xFF {
                self.flag = self.flag & 0b_0111_1111;
                self.size -= 1;
            }
        }

        self.pid = pid;
    }

    pub fn set_idempotent(&mut self, idempotent: u32) {
        assert!(idempotent > 0);

        let osize = if self.idempotent > 0xFFFF {
            4
        } else if self.idempotent > 0xFF {
            2
        } else {
            1
        };

        let nsize = if idempotent > 0xFFFF {
            if osize != 4 {
                self.flag = self.flag & 0b10011111 | Self::FLAG_IDEMPOTENT_32;
            }
            4
        } else if idempotent > 0xFF {
            if osize != 2 {
                self.flag = self.flag & 0b10011111 | Self::FLAG_IDEMPOTENT_16;
            }
            2
        } else {
            if osize != 1 {
                self.flag = self.flag & 0b10011111;
            }
            1
        };

        let res: i32 = nsize - osize;
        let size = res.abs() as usize;

        if res > 0 {
            self.size += size;
        } else {
            self.size -= size;
        }

        self.idempotent = idempotent;
    }

    /// 设置 用户ID
    pub fn set_router_id(&mut self, router_id: u64) {
        let osize = if self.router_id > 0xFFFFFFFF {
            8
        } else if self.router_id > 0xFFFF {
            4
        } else if self.router_id > 0xFF {
            2
        } else if self.router_id > 0 {
            1
        } else {
            0
        };

        let nsize = if router_id > 0xFFFFFFFF {
            if osize != 8 {
                self.flag = self.flag & 0b_111_000_11 | Self::FLAG_ROUTER_ID_64;
            }
            8
        } else if router_id > 0xFFFF {
            if osize != 4 {
                self.flag = self.flag & 0b_111_000_11 | Self::FLAG_ROUTER_ID_32;
            }
            4
        } else if router_id > 0xFF {
            if osize != 2 {
                self.flag = self.flag & 0b_111_000_11 | Self::FLAG_ROUTER_ID_16;
            }
            2
        } else if router_id > 0 {
            if osize != 1 {
                self.flag = self.flag & 0b_111_000_11 | Self::FLAG_ROUTER_ID_8;
            }
            1
        } else {
            if osize != 0 {
                self.flag = self.flag & 0b_111_000_11;
            }
            0
        };

        let res: i32 = nsize - osize;
        let size = res.abs() as usize;

        if res > 0 {
            self.size += size;
        } else {
            self.size -= size;
        }

        self.router_id = router_id;
    }

    /// 设置 消息体长度
    pub fn set_data_len(&mut self, data_len: usize) {
        let osize = if self.data_len > 0xFFFF {
            4
        } else if self.data_len > 0xFF {
            2
        } else if self.data_len > 0 {
            1
        } else {
            0
        };

        let nsize = if data_len > 0xFFFF {
            if osize != 4 {
                self.flag = self.flag & 0b11111100 | Self::FLAG_DATA_LEN_32;
            }
            4
        } else if data_len > 0xFF {
            if osize != 2 {
                self.flag = self.flag & 0b11111100 | Self::FLAG_DATA_LEN_16;
            }
            2
        } else if data_len > 0 {
            if osize != 1 {
                self.flag = self.flag & 0b11111100 | Self::FLAG_DATA_LEN_8;
            }
            1
        } else {
            if osize != 0 {
                self.flag = self.flag & 0b11111100
            }
            0
        };

        let res: i32 = nsize - osize;
        let size: usize = res.abs() as usize;
        if res > 0 {
            self.size += size;
        } else if res < 0 {
            self.size -= size;
        }

        self.data_len = data_len;
    }

    /// 获取消息头长度
    pub fn size(&self) -> usize {
        self.size
    }
}

#[derive(Debug)]
pub struct Package {
    head: Head,
    data_pos: usize,
    data_left: usize,
    data: Vec<u8>,
}

impl Package {
    pub fn new() -> Package {
        Package {
            head: Head::new(),

            data_pos: 0,
            data_left: 0,
            data: Vec::<u8>::new(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let nsize = self.head.size + self.head.data_len;
        let mut buf = vec![0u8; nsize];
        self.head.to_bytes(&mut buf[..self.head.size]);
        buf[self.head.size..nsize].copy_from_slice(&self.data);

        buf
    }

    pub fn data(&self) -> &[u8] {
        &self.data
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

    pub fn pid(&self) -> u16 {
        self.head.pid
    }

    pub fn idempotent(&self) -> u32 {
        self.head.idempotent
    }

    pub fn router_id(&self) -> u64 {
        self.head.router_id
    }

    pub fn data_len(&self) -> usize {
        self.head.data_len
    }

    pub fn parse(&mut self, buf: &[u8]) -> g::Result<bool> {
        self.head.parse(buf)?;

        let mut nsize = buf.len();

        if self.head.data_len >= self.data.capacity() {
            self.data.resize(self.head.data_len, 0);
        }

        if self.data_pos == 0 {
            self.data_left = self.head.data_len;
            nsize -= self.head.size;
            self.data[self.data_pos..].copy_from_slice(&buf[self.head.size..]);
        } else {
            self.data[self.data_pos..].copy_from_slice(&buf);
        }

        self.data_pos += nsize;

        Ok(self.data_pos > 0 && self.data_left == self.data_pos)
    }

    pub fn clear(&mut self) {
        self.data_pos = 0;
        self.data_left = 0;
        self.data.clear();
    }
}

#[cfg(test)]
mod pack_tester {
    use crate::utils;

    use super::{Head, Package};

    #[test]
    fn test_head() {
        // --------------------------------------
        let mut h1 = Head::new();

        assert_eq!(h1.size, Head::MIN_SIZE);
        assert_eq!(h1.flag_pid(), 1);

        h1.set_pid(0x10 /* 1字节数据 */);
        assert_eq!(h1.size, 3);
        assert_eq!(h1.flag_pid(), 1);

        h1.set_pid(0x100 /* 2字节数据 */);
        assert_eq!(h1.size, 4);
        assert_eq!(h1.flag_pid(), 2);

        h1.set_pid(0x10 /* 1字节数据 */);
        assert_eq!(h1.size, 3);
        assert_eq!(h1.flag_pid(), 1);

        // --------------------------------------
        let mut h2 = Head::new();
        assert_eq!(h2.flag_idempotent(), 1);

        h2.set_idempotent(0x100 /* 2字节数据 */);
        assert_eq!(h2.size, 4);
        assert_eq!(h2.flag_idempotent(), 2);

        h2.set_idempotent(0x10000 /* 4字节数据 */);
        assert_eq!(h2.size, 6);
        assert_eq!(h2.flag_idempotent(), 4);

        h2.set_idempotent(0x100 /* 2字节数据 */);
        assert_eq!(h2.size, 4);
        assert_eq!(h2.flag_idempotent(), 2);

        h2.set_idempotent(0x10 /* 1字节数据 */);
        assert_eq!(h2.size, 3);
        assert_eq!(h2.flag_idempotent(), 1);

        // --------------------------------------
        let mut h3 = Head::new();
        assert_eq!(h3.flag_router_id(), 0);

        h3.set_router_id(0x10 /* 1字节数据 */);
        assert_eq!(h3.size, 4);
        assert_eq!(h3.flag_router_id(), 1);

        h3.set_router_id(0x100 /* 2字节数据 */);
        assert_eq!(h3.size, 5);
        assert_eq!(h3.flag_router_id(), 2);

        h3.set_router_id(0x10000 /* 4字节数据 */);
        assert_eq!(h3.size, 7);
        assert_eq!(h3.flag_router_id(), 4);

        h3.set_router_id(0x100000000 /* 8字节数据 */);
        assert_eq!(h3.size, 11);
        assert_eq!(h3.flag_router_id(), 8);

        h3.set_router_id(0x10000 /* 4字节数据 */);
        assert_eq!(h3.size, 7);
        assert_eq!(h3.flag_router_id(), 4);

        h3.set_router_id(0x100 /* 2字节数据 */);
        assert_eq!(h3.size, 5);
        assert_eq!(h3.flag_router_id(), 2);

        h3.set_router_id(0x10 /* 1字节数据 */);
        assert_eq!(h3.size, 4);
        assert_eq!(h3.flag_router_id(), 1);

        h3.set_router_id(0 /* 0字节数据 */);
        assert_eq!(h3.size, 3);
        assert_eq!(h3.flag_router_id(), 0);

        // --------------------------------------
        let mut h4 = Head::new();
        assert_eq!(h4.flag_data_len(), 0);

        h4.set_data_len(0x10 /* 1字节数据 */);
        assert_eq!(h4.size, 4);
        assert_eq!(h4.flag_data_len(), 1);

        h4.set_data_len(0x100 /* 2字节数据 */);
        assert_eq!(h4.size, 5);
        assert_eq!(h4.flag_data_len(), 2);

        h4.set_data_len(0x10000 /* 4字节数据 */);
        assert_eq!(h4.size, 7);
        assert_eq!(h4.flag_data_len(), 4);

        h4.set_data_len(0x100 /* 2字节数据 */);
        assert_eq!(h4.size, 5);
        assert_eq!(h4.flag_data_len(), 2);

        h4.set_data_len(0x10 /* 1字节数据 */);
        assert_eq!(h4.size, 4);
        assert_eq!(h4.flag_data_len(), 1);

        h4.set_data_len(0 /* 0字节数据 */);
        assert_eq!(h4.size, 3);
        assert_eq!(h4.flag_data_len(), 0);

        // --------------------------------------
        let mut h5 = Head::new();
        h5.set_pid(0x10 /* 1字节数据 */);
        h5.set_idempotent(0x100 /* 2字节数据 */);
        h5.set_router_id(0x100000000 /* 8字节数据 */);
        h5.set_data_len(0x10 /* 1字节数据 */);

        assert_eq!(h5.size, 13);

        let mut buf = vec![0u8; h5.size];
        h5.to_bytes(&mut buf);

        let mut h6 = Head::new();
        if let Err(err) = h6.parse(&buf) {
            panic!("{:?}", err);
        }

        assert_eq!(h6.flag, h5.flag);
        assert_eq!(h6.pid, h5.pid);
        assert_eq!(h6.idempotent, h5.idempotent);
        assert_eq!(h6.router_id, h5.router_id);
        assert_eq!(h6.data_len, h5.data_len);
        assert_eq!(h6.size, h5.size);

        println!("{:?}", h6);
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
            assert_eq!(buf.len(), 17);

            let res = out_pack.parse(&buf).unwrap();
            assert!(res);

            assert_eq!(out_pack.pid(), in_pack.pid());
            assert_eq!(out_pack.idempotent(), in_pack.idempotent());
            assert_eq!(out_pack.router_id(), in_pack.router_id());
            assert_eq!(out_pack.data_len(), in_pack.data_len());

            assert_eq!(
                core::str::from_utf8(out_pack.data()).unwrap().to_string(),
                core::str::from_utf8(data).unwrap().to_string()
            );

            out_pack.clear();
        }

        println!("总耗时==: {} ms", utils::now_unix_mills() - beg);
    }
}

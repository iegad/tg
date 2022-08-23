// // ---------------------------
// // tg::nw::pck
// //     网络包定义
// //
// // @作者: iegad
// //
// // @时间: 2022-08-10
// // ---------------------------

// use crate::g;
// use core::slice::from_raw_parts;
// use lazy_static::lazy_static;
// use lockfree_object_pool::{LinearObjectPool, LinearReusable};
// use std::sync::Arc;

// /// 消息头
// ///
// /// 消息头为定长 20 字节
// ///
// /// # 内存布局
// /// | [service_id] 2字节 | [router_id] 4字节 | [package_id] 2字节 | [idempotent] 4字节 | [raw_len] 4字节 | [token] 4字节 |
// ///
// /// ## service_id 服务ID
// ///
// /// 每个服务都有一个 2字节ID
// ///
// /// ## router_id 路由ID
// ///
// /// 分布式中, 同一个service会有多个服务, router_id用于区分, 服务节点
// ///
// /// ## package_id 消息ID
// ///
// /// 用于区分消息包类型, 跟据此字段来处理消息请求
// ///
// /// ## idempotent 幂等
// ///
// /// 幂等, 用于检测包是否为重复包.
// ///
// /// ## raw_len
// ///
// /// 消息长度, 消息原始长度 消息头[10 bytes] + 消息体[N bytes].
// ///
// /// ## token
// ///
// /// 用于检测客户端是否合法
// pub struct Package {
//     raw: Vec<u8>,   //  原数据
//     raw_pos: usize, // 接收位置
// }

// pub type PackageItem<'a> = LinearReusable<'a, Package>;
// type PackagePool = LinearObjectPool<Package>;

// lazy_static! {
//     /// Package 对象池
//     ///
//     /// # Example
//     ///
//     /// ```
//     /// let mut p1 = tg::nw::pack::PACK_POOL.pull();
//     /// ```
//     pub static ref PACK_POOL: Arc<PackagePool> = {
//         let v = Arc::new(PackagePool::new(
//             || Package::new(),
//             |v| { v.raw_pos = 0; },
//         ));
//         v
//     };
// }

// impl Package {
//     /// Package 原始数据初始化长度.
//     /// 随着后期的调用, 原始数据长度为发生变化, 但决不会小于 [Pack::RAW_SIZE].
//     pub const DEFAULT_RAW_SIZE: usize = 4096;
//     pub const HEAD_SIZE: usize = 20;
//     const HEAD_KEY_16: u16 = 0xFBFA;
//     const HEAD_KEY_32: u32 = 0xFFFEFDFC;

//     /// 创建一个空包对象
//     pub fn new() -> Package {
//         Package {
//             raw: vec![0u8; Self::DEFAULT_RAW_SIZE],
//             raw_pos: 0,
//         }
//     }

//     /// 创建一个有初始化值的包对象
//     pub fn with_params(
//         service_id: u16,
//         router_id: u32,
//         package_id: u16,
//         idempotent: u32,
//         token: u32,
//         data: &[u8],
//     ) -> Package {
//         let raw_len = data.len() + Self::HEAD_SIZE;
//         let mut pack = Package {
//             raw: vec![0u8; raw_len],
//             raw_pos: 0,
//         };

//         pack.set_service_id(service_id);
//         pack.set_router_id(router_id);
//         pack.set_package_id(package_id);
//         pack.set_idempotent(idempotent);
//         pack.set_data(data);
//         pack.set_token(token);

//         pack
//     }

//     /// 获取码流的可写区域
//     pub fn as_mut_bytes(&mut self) -> &mut [u8] {
//         &mut self.raw[self.raw_pos..]
//     }

//     /// 将 self.raw[..n] 转换为 package.
//     ///
//     /// 成功转换为一个完整的包返回 true.
//     ///
//     /// 未成功转换为一个完整的包(后续还需要追加码流才能成功完整的Package) 返回 false.
//     ///
//     /// 无效的码流, 返回相应错误.
//     pub fn parse(&mut self, n: usize) -> g::Result<bool> {
//         if self.raw_pos == 0 {
//             if n < Self::HEAD_SIZE {
//                 return Err(g::Err::PackHeadInvalid("raw size is not long enough"));
//             }
//         }

//         let raw_len = self.raw_len();

//         // 当raw的长度不够时, 调整 raw大小
//         if raw_len > self.raw.capacity() {
//             self.raw.resize(raw_len, 0);
//         }

//         self.raw_pos += n;
//         let res = raw_len == self.raw_pos;

//         if res {
//             // 当前数据已经是完整的 Package时, 重置可写位置.
//             self.raw_pos = 0;
//         }

//         Ok(res)
//     }

//     /// 返回源始码流
//     pub fn to_bytes(&mut self) -> &[u8] {
//         &self.raw[..self.raw_len()]
//     }

//     /// 返回 service_id
//     pub fn service_id(&self) -> u16 {
//         (unsafe { *(self.raw.as_ptr() as *const u16) }) ^ Self::HEAD_KEY_16
//     }

//     /// 设置 pid
//     pub fn set_service_id(&mut self, service_id: u16) {
//         self.raw[..2].copy_from_slice(unsafe {
//             from_raw_parts(
//                 &(service_id ^ Self::HEAD_KEY_16) as *const u16 as *const u8,
//                 2,
//             )
//         });
//     }

//     pub fn router_id(&self) -> u32 {
//         (unsafe { *(self.raw.as_ptr().add(2) as *const u32) }) ^ Self::HEAD_KEY_32
//     }

//     pub fn set_router_id(&mut self, router_id: u32) {
//         self.raw[2..6].copy_from_slice(unsafe {
//             from_raw_parts(
//                 &(router_id ^ Self::HEAD_KEY_32) as *const u32 as *const u8,
//                 4,
//             )
//         });
//     }

//     pub fn package_id(&self) -> u16 {
//         (unsafe { *(self.raw.as_ptr().add(6) as *const u16) }) ^ Self::HEAD_KEY_16
//     }

//     pub fn set_package_id(&mut self, package_id: u16) {
//         self.raw[6..8].copy_from_slice(unsafe {
//             from_raw_parts(
//                 &(package_id ^ Self::HEAD_KEY_16) as *const u16 as *const u8,
//                 2,
//             )
//         });
//     }

//     /// 返回 幂等
//     pub fn idempotent(&self) -> u32 {
//         (unsafe { *(self.raw.as_ptr().add(8) as *const u32) }) ^ Self::HEAD_KEY_32
//     }

//     /// 设置 幂等
//     pub fn set_idempotent(&mut self, idempotent: u32) {
//         self.raw[8..12].copy_from_slice(unsafe {
//             from_raw_parts(
//                 &(idempotent ^ Self::HEAD_KEY_32) as *const u32 as *const u8,
//                 4,
//             )
//         });
//     }

//     /// 返回原始码流长度
//     pub fn raw_len(&self) -> usize {
//         ((unsafe { *(self.raw.as_ptr().add(12) as *const u32) }) ^ Self::HEAD_KEY_32) as usize
//     }

//     /// 获取消息体数据
//     pub fn data(&self) -> &[u8] {
//         &self.raw[Self::HEAD_SIZE..self.raw_len()]
//     }

//     /// 设置消息体数据
//     pub fn set_data(&mut self, data: &[u8]) {
//         let raw_len = data.len() + Self::HEAD_SIZE;

//         if raw_len > self.raw.capacity() {
//             self.raw.resize(raw_len, 0);
//         }

//         self.raw[12..16].copy_from_slice(unsafe {
//             from_raw_parts(
//                 &(raw_len as u32 ^ Self::HEAD_KEY_32) as *const u32 as *const u8,
//                 4,
//             )
//         });

//         self.raw[Self::HEAD_SIZE..raw_len].copy_from_slice(data);
//     }

//     pub fn token(&self) -> u32 {
//         (unsafe { *(self.raw.as_ptr().add(16) as *const u32) }) ^ Self::HEAD_KEY_32
//     }

//     pub fn set_token(&mut self, token: u32) {
//         self.raw[16..20].copy_from_slice(unsafe {
//             from_raw_parts(&(token ^ Self::HEAD_KEY_32) as *const u32 as *const u8, 4)
//         });
//     }
// }

// #[cfg(test)]
// mod pcomp_tester {
//     use super::Package;
//     use crate::utils;

//     #[test]
//     fn test_package() {
//         let s = "hello world";
//         let mut p1 = Package::with_params(0x01, 0x02, 0x03, 0x04, 0x05, s.as_bytes());
//         assert_eq!(p1.service_id(), 0x01);
//         assert_eq!(p1.router_id(), 0x02);
//         assert_eq!(p1.package_id(), 0x03);
//         assert_eq!(p1.idempotent(), 0x04);
//         assert_eq!(p1.token(), 0x05);
//         assert_eq!(p1.raw_len(), Package::HEAD_SIZE + s.len());
//         assert_eq!(s, core::str::from_utf8(p1.data()).unwrap());

//         let buf = p1.to_bytes();

//         println!("{}", utils::bytes_to_hex(buf));

//         let mut p2 = Package::new();
//         p2.as_mut_bytes()[..buf.len()].copy_from_slice(buf);

//         assert!(p2.parse(buf.len()).unwrap());

//         assert_eq!(p1.service_id(), p2.service_id());
//         assert_eq!(p1.router_id(), p2.router_id());
//         assert_eq!(p1.package_id(), p2.package_id());
//         assert_eq!(p1.idempotent(), p2.idempotent());
//         assert_eq!(p1.token(), p2.token());
//         assert_eq!(p1.raw_len(), p2.raw_len());
//         assert_eq!(
//             core::str::from_utf8(p1.data()).unwrap(),
//             core::str::from_utf8(p2.data()).unwrap()
//         );
//     }
// }

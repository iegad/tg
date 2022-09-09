use prost::bytes::BytesMut;
use tg::utils;

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Package {
    #[prost(uint32, tag = "1")]
    pub service_id: u32,
    #[prost(uint32, tag = "2")]
    pub package_id: u32,
    #[prost(uint32, tag = "3")]
    pub router_id: u32,
    #[prost(uint32, tag = "4")]
    pub idempotent: u32,
    #[prost(uint32, tag = "5")]
    pub raw_len: u32,
    #[prost(bytes = "vec", tag = "6")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}

fn main() {
    let beg = utils::now_unix_micros();
    let mut buf = BytesMut::new();

    for _ in 0..1000000 {
        let p = Package {
            service_id: 1,
            package_id: 2,
            router_id: 3,
            idempotent: 4,
            raw_len: 11,
            data: b"Hello world".to_vec(),
        };

        buf.reserve(prost::Message::encoded_len(&p));
        prost::Message::encode(&p, &mut buf).unwrap();

        let po = <Package as prost::Message>::decode(&mut buf).unwrap();
        assert_eq!(p.service_id, po.service_id);
        assert_eq!(p.package_id, po.package_id);
        assert_eq!(p.router_id, po.router_id);
        assert_eq!(p.idempotent, po.idempotent);
        assert_eq!(p.raw_len, po.raw_len);
        assert_eq!(
            std::str::from_utf8(&p.data).unwrap(),
            std::str::from_utf8(&po.data).unwrap()
        );
    }

    println!("prost 耗时: {}", utils::now_unix_micros() - beg);
}

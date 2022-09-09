use tg::{nw, utils};

fn main() {
    let beg = utils::now_unix_micros();
    for _ in 0..1000000 {
        let mut p = nw::pack::Package::new();
        p.set_service_id(1);
        p.set_package_id(2);
        p.set_idempotent(3);
        p.set_router_id(4);
        p.set_data(b"Hello world");

        let mut buf = p.to_bytes();
        let mut po = nw::pack::Package::new();
        po.from_buf(&mut buf).unwrap();

        assert_eq!(p.service_id(), po.service_id());
        assert_eq!(p.package_id(), po.package_id());
        assert_eq!(p.router_id(), po.router_id());
        assert_eq!(p.idempotent(), po.idempotent());
        assert_eq!(p.raw_len(), po.raw_len());
        assert_eq!(
            std::str::from_utf8(p.data()).unwrap(),
            std::str::from_utf8(po.data()).unwrap()
        );
    }
    println!("custom 耗时: {}", utils::now_unix_micros() - beg);
}

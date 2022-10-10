use criterion::{Criterion, criterion_group, criterion_main};

fn pack_raw() {
    let data = b"Hello world";
    for i in 0..100_000 {
        let p = tg::nw::pack::Package::with_params(1, i + 1, data);
        assert!(p.valid())
    } 
}

fn pack_pool() {
    let data = b"Hello world";
    for i in 0..100_000 {
        let mut p = tg::nw::pack::REQ_POOL.pull();
        p.set_package_id(1);
        p.set_idempotent(i + 1);
        p.set_data(data);
        p.active();

        assert!(p.valid());
    } 
}

fn pack_benchmark(c: &mut Criterion) {
    c.bench_function("pack_raw", |b|b.iter(pack_raw));
    c.bench_function("pack_pool", |b|b.iter(pack_pool));
}

criterion_group!(benches, pack_benchmark);
criterion_main!(benches);
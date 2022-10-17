// #[cfg(not(target_env = "msvc"))]
// use tikv_jemallocator::Jemalloc;

// #[cfg(not(target_env = "msvc"))]
// #[global_allocator]
// static GLOBAL: Jemalloc = Jemalloc;

use criterion::{Criterion, criterion_group, criterion_main};

fn vec_alloc() {
    for _ in 0..10000 {
        let v = vec![0u8; 65535];
        let _sum = v[0] + v[1];

    }
}

fn vec_benchmark(c: &mut Criterion) {
    c.bench_function("vec_alloc", |b|b.iter(vec_alloc));
}

criterion_group!(benches, vec_benchmark);
criterion_main!(benches);
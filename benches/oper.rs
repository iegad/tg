//! 三种方式性能都差不多. 但是位运算所出现的异常是最少的. 所以相对来说还是 位运算更优.

use criterion::{Criterion, criterion_group, criterion_main};
use rand::{Rng, thread_rng};

fn oper_add() {
    let mut rng = thread_rng();
    let num = rng.gen_range(1..100000);
    let v = num + num;
    for _ in 0..100_000 {
        assert_eq!(v, num + num);
    }
}

fn oper_move() {
    let mut rng = thread_rng();
    let num = rng.gen_range(1..100000);
    let v = num + num;
    for _ in 0..100_000 {
        assert_eq!(v, num << 1);
    }
}

fn oper_mul() {
    let mut rng = thread_rng();
    let num = rng.gen_range(1..100000);
    let v = num + num;
    for _ in 0..100_000 {
        assert_eq!(v, num * 2);
    }
}

fn pack_benchmark(c: &mut Criterion) {
    c.bench_function("oper_add", |b|b.iter(oper_add));
    c.bench_function("oper_move", |b|b.iter(oper_move));
    c.bench_function("oper_mul", |b|b.iter(oper_mul));
}

criterion_group!(benches, pack_benchmark);
criterion_main!(benches);
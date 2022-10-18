//! 该测试证明, map的方式不如pool在表现好.
// 猜测: 可能是因为即使是rwlock 也需要使用系统调用产生中断.

use std::sync::{Arc, RwLock};
use criterion::{Criterion, criterion_group, criterion_main};
use hashbrown::HashMap;
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;

lazy_static! {
    static ref POOL: LinearObjectPool<Arc<String>> = LinearObjectPool::new(||Arc::new(String::new()), |v| {
        unsafe { let p = &mut *(&**v as *const String as *mut String); p.clear(); }
    });
    static ref MAP: Arc<RwLock<HashMap<i32, Arc<String>>>> = Arc::new(RwLock::new(HashMap::new()));
}

fn data_from_rwl_map() {
    for i in 0..1_000_000 {
        let key = i % 10000;
        let item = {
            let option_s = {
                if let Some(v) = MAP.read().unwrap().get(&key) {
                    Some(v.clone())
                } else {
                    None
                }
            };

            if let Some(v) = option_s {
                v.clone()
            } else {
                let s = Arc::new(String::new());
                { MAP.write().unwrap().insert(key, s.clone()); }
                s
            }
        };

        assert!(item.is_empty());

        unsafe {
            let p = &mut *(&*item as *const String as *mut String);
            p.push_str("Hello world");
            assert!(item.len() > 0);
            p.clear();
        }
    }
}

fn data_from_pool() {
    for _ in 0..1_000_000 {
        let item = POOL.pull();
        assert!(item.is_empty());

        unsafe {
            let p = &mut *(&**item as *const String as *mut String);
            p.push_str("Hello world");
            assert!(item.len() > 0);
            p.clear();
        }
    }
}

fn map_benchmark(c: &mut Criterion) {
    c.bench_function("data_from_rwl_map", |b|b.iter(data_from_rwl_map));
    c.bench_function("data_from_pool", |b|b.iter(data_from_pool));
}

criterion_group!(benches, map_benchmark);
criterion_main!(benches);
# future::sync::mpsc VS tokio::sync::mpsc

测试结论为:

## 单生产者 - 单消费者

future::sync::mpsc 性能 是tokio::sync::mpsc 两倍以上.

## 多生产者 - 单消费者

future::sync::mpsc 和 tokio::sync::mpsc 不相上下.

# jemaclloc

ubuntu 20.04 LTS 测试中 性能比原始 allocator 提升 80%.
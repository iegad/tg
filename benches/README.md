# future::channel::mpsc VS tokio::sync::mpsc

## 单生产者 - 单消费者

future::channel::mpsc 性能 是tokio::sync::mpsc 两倍以上.

## 多生产者 - 单消费者

future::channel::mpsc 和 tokio::sync::mpsc 不相上下.

## 测试结论

由于多个生产者在框架中使用频率并不高, 这里指的是往管道中写数据. 所以部分情况是 "单生产者-单消费者" 方式, 所以框架中选择使用future::channel::mpsc 管道

# jemaclloc

ubuntu 20.04 LTS 测试中 性能比原始 allocator 提升 80%.
# tigen 平台

2022-09-26 框架在使用上确实不太友好, 并不是不想作出友好简单的调用, 但是RUST的语法就是这个 ...(无法形容的一言难尽)

后期对 RUST进行深度的学习, 并且在不牺牲功能与性能的情况下提升友好度.

2022-10-11 尽量确保框架代码的有效性, 并使用宏来提高友好度.

性能优化 https://nnethercote.github.io/perf-book/inlining.html

宏使用 https://blog.logrocket.com/macros-in-rust-a-tutorial-with-examples/

交叉编译 https://www.jianshu.com/p/6690476d4c3b

cargo bench --bench map_bench

使用 jemalloc 可以将分配空间的性能提升近一倍
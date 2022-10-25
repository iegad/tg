# tigen 平台

2022-09-26 框架在使用上确实不太友好, 并不是不想作出友好简单的调用, 但是RUST的语法就是这个 ...(无法形容的一言难尽)

后期对 RUST进行深度的学习, 并且在不牺牲功能与性能的情况下提升友好度.

2022-10-11 尽量确保框架代码的有效性, 并使用宏来提高友好度.

性能优化 https://nnethercote.github.io/perf-book/inlining.html

宏使用 https://blog.logrocket.com/macros-in-rust-a-tutorial-with-examples/

交叉编译 https://www.jianshu.com/p/6690476d4c3b

cargo bench --bench map_bench

使用 jemalloc 可以将分配空间的性能提升近一倍

cargo test [ut-name]

# echo_server - echo_client

SERVER: 
    CPU: 2 Cores 
    Mem: 2G 
    Net: 1Gbps

Clients: 50
Packets: 10000

公式: 并发数 / 响应时长 * CPU Cores
      
1. Total: 9883263  micro seconds.    QPS: 500,000 / (9.883263 * 2) = 25,295.2896 / s
2. Total: 10517879 micro seconds.    QPS: 500,000 / (10.517879 * 2) = 23,769.0508 / s
3. Total: 15877858 micro seconds.    QPS: 500,000 / (15.877858 * 2) = 15,745.1969 / s
4. Total: 8654372  micro seconds.    QPS: 500,000 / (8.654372 * 2) = 28,887.1336 / s
4. Total: 9369455  micro seconds.    QPS: 500,000 / (9.369455 * 2) = 26,682.4484 / s

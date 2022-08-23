// use super::{pack, IProc, Server, CONN_POOL};
// use crate::g;
// use std::net::SocketAddr;
// use tokio::{
//     io::{AsyncReadExt, AsyncWriteExt},
//     net::{TcpListener, TcpStream},
//     select, signal,
//     sync::broadcast,
// };

// /// # run
// ///
// /// 运行server
// pub async fn run<TProc>(server: &Server, proc: TProc)
// where
//     TProc: IProc,
// {
//     if let Err(err) = proc.on_init(server) {
//         println!("proc.on_init failed: {:?}", err);
//         return;
//     }

//     let listener = match TcpListener::bind(server.host).await {
//         Ok(l) => l,
//         Err(err) => panic!("tcp server bind failed: {:?}", err),
//     };

//     let (notify_shutdown, mut shutdown) = broadcast::channel(1);

//     'server_loop: loop {
//         let permit = server
//             .limit_connections
//             .clone()
//             .acquire_owned()
//             .await
//             .unwrap();

//         select! {
//             // 接收连接句柄
//             res_accept = listener.accept() => {
//                 let (stream, addr) = match res_accept {
//                     Ok((s, addr)) => (s,addr),
//                     Err(err) => {
//                         println!("accept failed: {:?}", err);
//                         break 'server_loop;
//                     }
//                 };

//                 let shutdown = notify_shutdown.subscribe();

//                 tokio::spawn(async move {
//                     conn_handle(stream, addr, proc, shutdown).await;
//                     drop(permit);
//                 });
//             }

//             // SIGINT 信号句柄
//             _ = signal::ctrl_c() => {
//                 if let Err(err) = notify_shutdown.send(1) {
//                     panic!("shutdown_sender.send failed: {:?}", err);
//                 }
//             }

//             // 关停句柄
//             _ = shutdown.recv() => {
//                 break 'server_loop;
//             }
//         }
//     }

//     proc.on_released(server);
// }

// async fn conn_handle<TProc>(
//     mut stream: TcpStream,
//     _addr: SocketAddr,
//     proc: TProc,
//     mut notify_shutdown: broadcast::Receiver<u8>,
// ) where
//     TProc: IProc,
// {
//     let mut conn = CONN_POOL.pull();
//     if let Err(err) = conn.init(&stream) {
//         println!("conn.init failed: {:?}", err);
//         return;
//     };

//     if let Err(err) = proc.on_connected(&*conn) {
//         println!("proc.on_connected failed: {:?}", err);
//         return;
//     }

//     let (mut reader, mut writer) = stream.split();
//     let (wch_sender, mut wch_receiver) = tokio::sync::mpsc::channel(g::DEFAULT_CHAN_SIZE);
//     let mut timeout_ticker =
//         tokio::time::interval(std::time::Duration::from_secs(g::DEFAULT_READ_TIMEOUT));

//     'conn_loop: loop {
//         timeout_ticker.reset();

//         select! {
//             // 关停句柄
//             _ = notify_shutdown.recv() => {
//                 println!("server is shutdown");
//                 break 'conn_loop;
//             }

//             // 超时句柄
//             _ = timeout_ticker.tick() => {
//                 proc.on_conn_error(&*conn, g::Err::TcpReadTimeout);
//                 break 'conn_loop;
//             }

//             // 消息发送句柄
//             result_rsp = wch_receiver.recv() => {
//                 let mut rsp: pack::PackageItem = match result_rsp {
//                     None => panic!("failed wch rsp"),
//                     Some(v) => v,
//                 };

//                 if let Err(err) = writer.write_all(rsp.to_bytes()).await {
//                     proc.on_conn_error(&*conn, g::Err::TcpWriteFailed(format!("write failed: {:?}", err)));
//                     break 'conn_loop;
//                 }

//                 conn.send_seq_incr();
//             }

//             // 消息接收句柄
//             result_read = reader.read(conn.req_mut().as_mut_bytes()) => {
//                 let n = match result_read {
//                     Ok(0) => {
//                         println!("conn[{}:{:?}] closed", conn.sockfd(), conn.remote());
//                         break 'conn_loop;
//                     }

//                     Ok(n) => n,

//                     Err(err) => {
//                         proc.on_conn_error(&*conn, g::Err::TcpReadFailed(format!("read failed: {:?}", err)));
//                         break 'conn_loop;
//                     }
//                 };

//                 let ok = match conn.req_mut().parse(n) {
//                     Err(err) => {
//                         proc.on_conn_error(&*conn, err);
//                         break 'conn_loop;
//                     }

//                     Ok(v) => v,
//                 };

//                 if ok {
//                     conn.recv_seq_incr();
//                     let rsp = match proc.on_process(&*conn) {
//                         Err(_) => break 'conn_loop,
//                         Ok(rsp) => rsp,
//                     };

//                     if let Err(err) = wch_sender.send(rsp).await {
//                         panic!("write channel[mpsc] send failed: {}", err);
//                     }
//                 }
//             }
//         }
//     }
//     proc.on_disconnected(&*conn);
// }

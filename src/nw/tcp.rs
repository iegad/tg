use super::{pack, Conn, IEvent, IServerEvent, Server};
use crate::{g, us::Ptr};
use lazy_static::lazy_static;
use lockfree_object_pool::LinearObjectPool;
use std::sync::{atomic::Ordering, Arc};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream},
    select,
    sync::broadcast,
};

lazy_static! {
    static ref CONN_POOL: LinearObjectPool<Conn> =
        LinearObjectPool::new(|| Conn::new(), |v| { v.reset() });
    static ref REQ_POOL: LinearObjectPool<pack::Package> =
        LinearObjectPool::new(|| pack::Package::new(), |v| { v.reset() });
}

pub async fn server_run<T: IServerEvent>(server: Arc<Ptr<Server<T>>>) -> io::Result<()> {
    let lfd = TcpSocket::new_v4()?;
    lfd.set_reuseport(true)?;
    lfd.set_reuseaddr(true)?;
    lfd.bind(server.host().parse().unwrap())?;

    let listener = lfd.listen(1024)?;
    let notify_shutdown = server.shutdown.clone();
    let mut shutdown = server.shutdown.subscribe();

    server.event.on_runing(server.clone()).await;
    server.running.store(true, Ordering::SeqCst);

    'accept_loop: loop {
        select! {
            result_accept =  listener.accept() => {
                let (stream, _) =  result_accept?;
                let permit = server.limit_connections.clone().acquire_owned().await.unwrap();
                let event = server.event.clone();
                let shutdown = notify_shutdown.subscribe();
                let timeout = server.timeout;

                tokio::spawn(async move {
                    conn_handle(stream, timeout, shutdown, event, permit).await;
                });
            }

            _ = shutdown.recv() => {
                break 'accept_loop;
            }
        }
    }

    server.get_mut().running.store(false, Ordering::SeqCst);
    server.event.on_stopped(server.clone()).await;

    Ok(())
}

pub async fn conn_handle<T: IEvent>(
    mut stream: TcpStream,
    timeout: u64,
    mut shutdown: broadcast::Receiver<u8>,
    event: T,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    let mut conn = CONN_POOL.pull();
    conn.load_from(&stream);

    let (mut reader, mut writer) = stream.split();
    let tx = conn.sender();
    let mut rx = conn.receiver();
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(timeout));
    let mut req = REQ_POOL.pull();

    if let Err(_) = event.on_connected(&conn).await {
        return;
    }

    'conn_loop: loop {
        ticker.reset();

        select! {
            _ = shutdown.recv() => {
                break 'conn_loop;
            }

            _ = ticker.tick() => {
                event.on_error(&conn, g::Err::TcpReadTimeout).await;
                break 'conn_loop;
            }

            result_wbuf = rx.recv() => {
                let wbuf = match result_wbuf {
                    Err(err) => panic!("wch recv failed: {:?}", err),
                    Ok(v) => v,
                };

                if let Err(err) = writer.write_all(&wbuf).await {
                    event.on_error(&conn, g::Err::TcpWriteFailed(format!("{:?}", err))).await;
                    break;
                }
                conn.send_seq += 1;
            }

            result_read = reader.read_buf(conn.rbuf_mut()) => {
                match result_read {
                    Err(err) => {
                        event.on_error(&conn, g::Err::TcpReadFailed(format!("{:?}", err))).await;
                        break 'conn_loop;
                    }
                    Ok(0) => break 'conn_loop,
                    Ok(_) => (),
                }

                loop {
                    if req.valid() {
                        conn.recv_seq += 1;
                        let option_rsp = match event.on_process(&conn, &req).await {
                            Err(_) => break 'conn_loop,
                            Ok(v) => v,
                        };

                        req.reset();
                        if let Some(rsp) = option_rsp {
                            tx.send(rsp).unwrap();
                        }
                    } else {
                        if let Err(err) = pack::Package::parse(conn.rbuf_mut(), &mut req) {
                            event.on_error(&conn, err).await;
                            break 'conn_loop;
                        }
                    }

                    if !req.valid() && conn.rbuf_mut().len() < pack::Package::HEAD_SIZE {
                        break;
                    }
                }

                conn.check_rbuf();
            }
        }
    }

    drop(permit);
    event.on_disconnected(&conn).await;
}

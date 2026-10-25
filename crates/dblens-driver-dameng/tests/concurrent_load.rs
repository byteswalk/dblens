//! 回归测试：dmdpi 在 DLL 初始化时调用 getaddrinfo。若此时另一个线程正在进程内第一次创建 socket，
//! 两边会在 Windows 加载器锁和 Winsock 目录锁上互相等待，整个进程卡死。
//! 必须是独立的测试程序：进程里此前不能创建过任何 socket。

use std::net::TcpStream;
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::Duration;

use dblens_driver::{ConnectParams, Driver};
use dblens_driver_dameng::DmDriver;

#[test]
fn loading_dpi_while_another_thread_opens_first_socket_does_not_deadlock() {
    let Ok(url) = std::env::var("DBLENS_TEST_DM_URL") else { return };
    let (_, params) = ConnectParams::from_url(&url).unwrap();
    let addr = format!("{}:{}", params.host, params.port);
    let barrier = Arc::new(Barrier::new(2));
    let (done, finished) = mpsc::channel();

    let (b, tx) = (barrier.clone(), done.clone());
    thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        b.wait();
        let ok = rt.block_on(DmDriver.connect(&params)).is_ok();
        tx.send(("达梦连接", ok)).unwrap();
    });
    thread::spawn(move || {
        barrier.wait();
        let ok = TcpStream::connect(addr).is_ok();
        done.send(("TCP 连接", ok)).unwrap();
    });

    for _ in 0..2 {
        match finished.recv_timeout(Duration::from_secs(60)) {
            Ok((who, ok)) => assert!(ok, "{who} 失败"),
            Err(_) => {
                // 死锁的线程占着加载器锁，正常退出进程也会卡住，只能直接中止
                eprintln!("60 秒内没有完成：加载 dmdpi 与创建 socket 互相死锁");
                std::process::abort();
            }
        }
    }
}

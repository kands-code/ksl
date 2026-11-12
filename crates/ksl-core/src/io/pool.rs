//! 阻塞 I/O 工作线程池.
//!
//! 文件与目录操作可能阻塞, 统一交给固定数量的工作线程; 标准输入读取走
//! 专用输入线程, 不占用这里的线程.

use std::sync::LazyLock;
use std::thread;

use crossbeam_channel::{Sender, unbounded};

const WORKERS: usize = 4;

type Job = Box<dyn FnOnce() + Send + 'static>;

struct Pool {
    jobs: Sender<Job>,
}

static POOL: LazyLock<Pool> = LazyLock::new(Pool::start);

impl Pool {
    fn start() -> Self {
        let (jobs, receiver) = unbounded::<Job>();
        for index in 0..WORKERS {
            let receiver = receiver.clone();
            thread::Builder::new()
                .name(format!("ksl-io-{index}"))
                .spawn(move || {
                    for job in receiver {
                        job();
                    }
                })
                .expect("无法启动 I/O 工作线程");
        }
        Self { jobs }
    }
}

pub(super) fn submit(job: Job) {
    let _ = POOL.jobs.send(job);
}

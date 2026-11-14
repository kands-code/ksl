//! 阻塞 I/O 工作线程池.
//!
//! 文件与目录操作可能阻塞, 统一交给固定数量的工作线程; 标准输入读取走
//! 专用输入线程, 不占用这里的线程. 数量默认按 CPU 核数在上下限之间取定,
//! 命令行可用 `--io-workers` 覆盖; 线程池只在首次提交时启动, 启动后配置不再生效.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use crossbeam_channel::{Sender, unbounded};

/// 默认线程数的下限; 低核数机器也保留基本并发能力.
const DEFAULT_MIN_WORKERS: usize = 4;

/// 默认线程数的上限; 阻塞线程过多不增益, 只增调度噪声.
const DEFAULT_MAX_WORKERS: usize = 16;

/// 显式配置的线程数; 0 表示未配置.
static CONFIGURED: AtomicUsize = AtomicUsize::new(0);

type Job = Box<dyn FnOnce() + Send + 'static>;

struct Pool {
    jobs: Sender<Job>,
}

static POOL: LazyLock<Pool> = LazyLock::new(Pool::start);

/// 配置线程数; 线程池已启动后调用不再生效.
pub(super) fn configure(count: usize) {
    CONFIGURED.store(count.max(1), Ordering::Relaxed);
}

/// 线程池启动时的实际线程数.
fn worker_count() -> usize {
    let configured = CONFIGURED.load(Ordering::Relaxed);
    if configured > 0 {
        return configured;
    }
    thread::available_parallelism().map_or(DEFAULT_MIN_WORKERS, |cpus| {
        cpus.get().clamp(DEFAULT_MIN_WORKERS, DEFAULT_MAX_WORKERS)
    })
}

impl Pool {
    fn start() -> Self {
        let (jobs, receiver) = unbounded::<Job>();
        for index in 0..worker_count() {
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

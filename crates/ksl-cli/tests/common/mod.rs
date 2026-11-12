//! 测试临时目录.
//!
//! 固定放在 Cargo 提供的 `CARGO_TARGET_TMPDIR`, 即 `target/tmp`, 随 `cargo clean`
//! 清理. 目录名带进程号与序号, 并发测试互不覆盖. 进程被强制中断时 Drop 不执行,
//! 残留只会留在 `target/tmp`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(crate) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 在 `target/tmp` 下创建带前缀的唯一目录.
    #[must_use]
    pub(crate) fn new(prefix: &str) -> Self {
        let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        fs::create_dir_all(&base).expect("临时根目录应可创建");
        loop {
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("{prefix}{}-{serial}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
                Err(error) => panic!("临时目录应可创建: {error}"),
            }
        }
    }

    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // 子进程可能仍持有目录; 删除失败没有补救手段, 忽略.
        let _ = fs::remove_dir_all(&self.path);
    }
}

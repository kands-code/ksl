//! 测试临时目录与文件写入.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// 测试临时目录.
///
/// 单元测试无法取得 `CARGO_TARGET_TMPDIR`, 按工作区布局回退到 `target/tmp`.
/// 目录名带进程号与序号, 并发测试互不覆盖.
/// 进程被强制中断时 Drop 不执行, 残留只会留在 `target/tmp`.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 在 `target/tmp` 下创建带前缀的唯一目录.
    ///
    /// # Panics
    ///
    /// 临时根目录或唯一子目录无法创建时 panic.
    #[must_use]
    pub fn new(prefix: &str) -> Self {
        let base = target_tmp();
        fs::create_dir_all(&base).expect("临时根目录应可创建");
        loop {
            let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = base.join(format!("{prefix}{}-{serial}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {},
                Err(error) => panic!("临时目录应可创建: {error}"),
            }
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // 子进程可能仍持有目录; 删除失败没有补救手段, 忽略.
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn target_tmp() -> PathBuf {
    option_env!("CARGO_TARGET_TMPDIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"),
        PathBuf::from,
    )
}

/// 在临时目录中写入文件, 自动创建父目录.
pub fn write(root: &TempDir, relative: &str, text: &str) {
    let _ = write_path(root, relative, text);
}

/// 在临时目录中写入文件并返回路径, 自动创建父目录.
///
/// # Panics
///
/// 目录或文件无法创建时 panic.
#[must_use]
pub fn write_path(root: &TempDir, relative: &str, text: &str) -> PathBuf {
    let path = root.path().join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("模块目录应可创建");
    }
    fs::write(&path, text).expect("模块文件应可写入");
    path
}

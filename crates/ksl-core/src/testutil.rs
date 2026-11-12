//! 测试支撑: 脚本装载/检查入口与 target/tmp 临时目录.
//!
//! 脚本源码用原始字符串书写, 免去引号与花括号的转义; 各阶段测试共用
//! 同一组入口, 避免每个测试重复搭台.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ast::Program;
use crate::error::KslError;
use crate::source::Sources;

/// 解析脚本源码.
pub fn parse(source: &str) -> Result<Program, KslError> {
    let mut sources = Sources::default();
    let id = sources.add("<test>", source);
    crate::parser::parse_text(sources.text(id), id)
}

/// 类型检查脚本源码.
pub fn check(source: &str) -> Result<(), KslError> {
    let program = parse(source).expect("脚本应可解析");
    crate::check::check_program(&program)
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// 测试临时目录.
///
/// 单元测试拿不到 `CARGO_TARGET_TMPDIR`, 按工作区布局回退到 `target/tmp`;
/// 目录名带进程号与序号, 并发测试互不覆盖. 进程被强制中断时 Drop 不执行,
/// 残留只会留在 `target/tmp`.
pub(crate) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 在 `target/tmp` 下创建带前缀的唯一目录.
    #[must_use]
    pub(crate) fn new(prefix: &str) -> Self {
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

fn target_tmp() -> PathBuf {
    option_env!("CARGO_TARGET_TMPDIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp"),
        PathBuf::from,
    )
}

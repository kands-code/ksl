//! 构建脚本: 编译 tree-sitter 生成的解析器与 ksl 的外部扫描器.
//!
//! `src/parser.c` 与 `src/tree_sitter/parser.h` 由 tree-sitter CLI 生成后入库,
//! 故本仓库不依赖 tree-sitter CLI 即可构建; 重新生成见 `README.md`.

use std::path::PathBuf;

fn main() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut build = cc::Build::new();
    build
        .std("c11")
        .include(&src)
        .file(src.join("parser.c"))
        .file(src.join("scanner.c"))
        .warnings(false)
        .compile("tree-sitter-ksl");

    println!("cargo:rerun-if-changed={}", src.join("parser.c").display());
    println!("cargo:rerun-if-changed={}", src.join("scanner.c").display());
}

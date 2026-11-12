//! 模块加载与执行的端到端测试.

use std::fs;

use common::{command, run_in, stderr, stdout};
use ksl_base::testutil::TempDir;

mod common;

fn temp_root() -> TempDir {
    let root = TempDir::new("ksl_cli_module_");
    fs::create_dir_all(root.path().join("lib")).expect("模块目录应可创建");
    root
}

#[test]
fn run_executes_module_program() {
    let root = temp_root();
    fs::write(
        root.path().join("lib/a.ksl"),
        r#"
@Data[A, {C[i64]}];
@Let[make, @Fn[[n : i64] : A, A/C[n]]];
@Let[show, @Fn[[v : A] : string, @Match[v, { (/C[n], f"{n}") }]]];
@Export[make, show, ^A];
"#,
    )
    .expect("依赖模块应可写入");
    fs::write(
        root.path().join("main.ksl"),
        r#"
@Use[{make, show}, "lib/a"];
@Let[v, make[42]];
Print[StringJoin[{show[v], "\n"}]];
nil
"#,
    )
    .expect("主模块应可写入");
    let output = command()
        .arg("run")
        .arg(root.path().join("main.ksl"))
        .output()
        .expect("命令行应可启动");
    assert!(output.status.success(), "模块执行失败: {}", stderr(&output));
    assert_eq!(stdout(&output), "42\n");
}

#[test]
fn main_exit_sets_process_exit_code() {
    let root = temp_root();
    let path = root.path().join("main.ksl");
    fs::write(&path, "@Main[[self : Pid[nil, nil]] : nil, @Exit[5u8]];")
        .expect("主模块应可写入");
    let output = command()
        .arg("run")
        .arg(&path)
        .output()
        .expect("命令行应可启动");
    assert_eq!(
        output.status.code(),
        Some(5),
        "@Main 内的 @Exit 应传达到进程: {}",
        stderr(&output)
    );
}

#[test]
fn run_loads_transitive_modules_from_code() {
    let root = temp_root();
    fs::write(
        root.path().join("lib/a.ksl"),
        r"
@Def[f, [n : i64] : i64, AddI64[n, 1]];
@Export[f];
",
    )
    .expect("依赖模块应可写入");
    fs::write(
        root.path().join("lib/b.ksl"),
        r#"
@Use[{f}, "a"];
@Let[g, @Fn[[n : i64] : i64, MulI64[f[n], f[n]]]];
@Export[g];
"#,
    )
    .expect("模块应可写入");
    let output = run_in(
        root.path(),
        &[
            "run",
            "--code",
            r#"
@Use[{g}, "lib/b"];
@Let[x, g[20]];
Print[f"{x}\n"]
"#,
        ],
        "",
    );
    assert!(output.status.success(), "模块执行失败: {}", stderr(&output));
    assert_eq!(stdout(&output), "441\n");
}

#[test]
fn check_uses_project_root_from_startup_directory() {
    let root = temp_root();
    fs::write(root.path().join(".kslproj"), "").expect("项目标记应可写入");
    fs::write(
        root.path().join("util.ksl"),
        r"@Let[g, @Fn[[] : i64, 1]]; @Export[g];",
    )
    .expect("依赖应可写入");
    fs::write(
        root.path().join("lib/a.ksl"),
        r#"@Use[{g}, "../util"]; @Let[f, @Fn[[] : i64, g[]]];"#,
    )
    .expect("文档应可写入");
    let output = run_in(root.path(), &["check", "lib/a.ksl"], "");
    assert!(
        output.status.success(),
        "项目根下应通过检查: {}",
        stderr(&output)
    );
}

#[test]
fn check_without_project_marker_uses_file_directory() {
    let root = temp_root();
    fs::write(
        root.path().join("util.ksl"),
        r"@Let[g, @Fn[[] : i64, 1]]; @Export[g];",
    )
    .expect("依赖应可写入");
    fs::write(
        root.path().join("lib/a.ksl"),
        r#"@Use[{g}, "../util"]; @Let[f, @Fn[[] : i64, g[]]];"#,
    )
    .expect("文档应可写入");
    let output = run_in(root.path(), &["check", "lib/a.ksl"], "");
    assert!(!output.status.success(), "无标记时应按文件目录检查");
    assert!(
        stderr(&output).contains("模块路径超出根目录"),
        "应报告路径越界: {}",
        stderr(&output)
    );
}

#[test]
fn run_uses_file_directory_even_with_project_marker() {
    let root = temp_root();
    fs::write(root.path().join(".kslproj"), "").expect("项目标记应可写入");
    fs::write(
        root.path().join("util.ksl"),
        r"@Let[g, @Fn[[] : i64, 1]]; @Export[g];",
    )
    .expect("依赖应可写入");
    fs::write(
        root.path().join("lib/a.ksl"),
        r#"@Use[{g}, "../util"]; @Let[f, @Fn[[] : i64, g[]]];"#,
    )
    .expect("文档应可写入");
    let output = run_in(root.path(), &["run", "lib/a.ksl"], "");
    assert!(!output.status.success(), "run 应以文件目录为根");
    assert!(
        stderr(&output).contains("模块路径超出根目录"),
        "应报告路径越界: {}",
        stderr(&output)
    );
}

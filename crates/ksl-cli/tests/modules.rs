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
fn run_loads_transitive_modules() {
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
    fs::write(
        root.path().join("main.ksl"),
        r#"
@Use[{g}, "lib/b"];
@Let[x, g[20]];
Print[f"{x}\n"]
"#,
    )
    .expect("主模块应可写入");
    let output = command()
        .arg("run")
        .arg(root.path().join("main.ksl"))
        .output()
        .expect("命令行应可启动");
    assert!(output.status.success(), "模块执行失败: {}", stderr(&output));
    assert_eq!(stdout(&output), "441\n");
}

#[test]
fn code_input_can_import_modules() {
    let root = temp_root();
    fs::write(
        root.path().join("lib/a.ksl"),
        r"
@Data[A, {C[i64]}];
@Let[make, @Fn[[n : i64] : A, A/C[n]]];
@Export[make, ^A];
",
    )
    .expect("依赖模块应可写入");
    let output = run_in(
        root.path(),
        &[
            "run",
            "--code",
            r#"
@Use[{make, ^A}, "lib/a"];
@Let[v, make[9]];
@Let[n, @Match[v, {(A/C[x], x)}]];
Print[f"{n}\n"];
"#,
        ],
        "",
    );
    let (out, err) = (stdout(&output), stderr(&output));
    assert!(out.contains('9'), "命令行代码应可导入模块: {out}{err}");
}

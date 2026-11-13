//! `cps::ir` 模块测试.

use super::*;

fn lower(source: &str) -> String {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = crate::check::Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    lower_program(&program, session.types()).to_string()
}

#[test]
fn tail_and_non_tail_calls_are_distinct() {
    let ir = lower(
        r"
        @Def[count, [n : i64] : i64,
          @If[LessOrEqualI64[n, 0], 0, count[SubI64[n, 1]]]];
        @Def[sum, [n : i64] : i64,
          @If[LessOrEqualI64[n, 0], 0, AddI64[1, sum[SubI64[n, 1]]]]];
        count[10]
    ",
    );
    assert!(ir.contains("tailcall count("));
    assert!(ir.contains("call sum("));
}

#[test]
fn actor_primitives_are_explicit() {
    let ir = lower(
        r"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[worker, @Handler[[self : Pid[i64, i64], n : i64] : i64,
              @Match[@Receive[0u, Option/None[]], {
                (/Some[message], @Continue[message]),
                (/None[], n)
              }]]],
            @Let[_p, @Spawn[worker, 0]]
          ]]
    ",
    );
    assert!(ir.contains("handler $handler1(self : Pid[i64, i64], n : i64, $k) : i64:"));
    assert!(ir.contains("receive 0,"));
    assert!(ir.contains("continue(%"));
    assert!(ir.contains("prim @Spawn("));
    assert!(
        ir.contains(": Option[u64] = Option/None[]"),
        "超时绑定应为 Option[u64]: {ir}"
    );
    assert!(
        !ir.contains(": Option[i64] = Option/None[]"),
        "超时绑定不应写成 Option[i64]: {ir}"
    );
}

#[test]
fn data_constructors_and_list_ops_are_visible() {
    let ir = lower(
        r#"
        @Data[Shape, {Circle[f64], Point[]}];
        @Let[shapes, {Shape/Circle[1.0]}];
        @ListMap[shapes, @Fn[[shape : Shape] : string, "ok"]]
    "#,
    );
    assert!(ir.contains("data Shape {"));
    assert!(ir.contains("Circle[f64]"));
    assert!(ir.contains("Shape/Circle["));
    assert!(ir.contains("prim @ListMap("));
}

#[test]
fn synthetic_values_and_continuations_use_stable_ids() {
    let ir = lower("@Let[a, 1]; @Let[r, AddI64[a, 2]]; r");
    assert!(ir.contains("let %3 : i64 = 1"));
    assert!(ir.contains("global let a : i64 = %3"));
    assert!(ir.contains("call AddI64(%1, %2) then $k0(%0 : i64):"));
    assert!(ir.contains("global let r : i64 = %0"));
}

#[test]
fn long_list_formats_without_recursion() {
    // CPS 文本输出同样按树深递归; 用长列表覆盖显式栈格式化路径.
    let items = (0..5000).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let ir = lower(&format!("@Let[xs, {{{items}}}]; xs"));
    assert!(ir.contains("global let xs"), "长列表应完成 CPS 格式化");
}

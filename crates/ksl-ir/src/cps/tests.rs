//! CPS IR 模块测试.

fn lower(source: &str) -> String {
    let program = ksl_frontend::testutil::parse(source).expect("脚本应可解析");
    let mut session = ksl_frontend::check::Session::new();
    let typed = session.check_typed(&program, None).expect("脚本应通过类型检查");
    crate::lower::to_cps(&typed).to_string()
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
      @Match[@Receive[0u8, Option/None[]], {
        (/Some[message], @Continue[message]),
        (/None[], n)
      }]]],
    @Let[_, @Spawn[worker, 0]]
  ]]
",
    );
    assert!(ir.contains("handler $handler"), "缺少 Handler 函数: {ir}");
    assert!(
        ir.contains("(self : Pid[i64, i64], n : i64, $k) : i64:"),
        "Handler 签名不符: {ir}"
    );
    assert!(ir.contains("receive 0,"));
    assert!(ir.contains("continue(message)"));
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
fn long_list_formats_without_recursion() {
    // CPS 文本输出不按树深递归; 用长列表覆盖显式栈格式化路径.
    let items = (0..5000).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let ir = lower(&format!("@Let[xs, {{{items}}}]; xs"));
    assert!(ir.contains("global let xs"), "长列表应完成 CPS 格式化");
}

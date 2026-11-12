//! `cps::exec` 模块测试.

use super::*;
use crate::symbol::intern;

fn run(source: &str) -> Result<Value, Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = crate::check::Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    Machine::new().run_program(&program, session.types())
}

fn run_on(
    session: &mut crate::check::Session,
    machine: &mut Machine,
    source: &str,
) -> Result<Value, Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    machine.run_program(&program, session.types())
}

fn run_on_int(session: &mut crate::check::Session, machine: &mut Machine, source: &str) -> i64 {
    match run_on(session, machine, source) {
        Ok(Value::I64(value)) => value,
        other => panic!("结果不是整数: {other:?}"),
    }
}

#[test]
fn globals_follow_program_boundaries() {
    // 失败的输入整段回滚.
    let program = crate::testutil::parse("@Let[x, AddI64[9223372036854775807, 1]];")
        .expect("脚本应可解析");
    let mut session = crate::check::Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut machine = Machine::new();
    assert!(machine.run_program(&program, session.types()).is_err());
    assert!(!machine.globals.contains_key(&intern("x")));

    // 块内绑定不进入全局环境.
    let mut session = crate::check::Session::new();
    let mut machine = Machine::new();
    assert_eq!(
        run_on_int(
            &mut session,
            &mut machine,
            "@Let[x, 1]; @Block[@Let[y, 2], AddI64[x, 1]]"
        ),
        2
    );
    assert!(!machine.globals.contains_key(&intern("y")));
}

#[test]
fn top_level_bindings_survive_tail_calls_across_inputs() {
    let mut session = crate::check::Session::new();
    let mut machine = Machine::new();
    assert_eq!(
        run_on_int(&mut session, &mut machine, "@Let[x, 1];\nAddI64[x, 2]"),
        3
    );
    assert_eq!(run_on_int(&mut session, &mut machine, "x"), 1);
    assert_eq!(
        run_on_int(
            &mut session,
            &mut machine,
            "@Def[f, [n : i64] : i64, AddI64[n, 1]]; f[41]"
        ),
        42
    );
    assert_eq!(run_on_int(&mut session, &mut machine, "f[0]"), 1);
    assert!(matches!(
        run_on(&mut session, &mut machine, "@Let[z, 9]"),
        Ok(Value::Nil)
    ));
    assert_eq!(run_on_int(&mut session, &mut machine, "z"), 9);
}

#[test]
fn runtime_error_span_points_at_expression() {
    let source = "@Let[x, DivideI64[1, 0]];";
    let Error::Runtime(error) = run(source).expect_err("应报运行时错误") else {
        panic!("应为运行时错误");
    };
    let span = error.span.expect("运行时错误应带位置");
    assert_eq!(&source[span.start..span.end], "DivideI64[1, 0]");
}

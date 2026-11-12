//! `runtime::machine` 模块测试.
//!
//! 跨输入会话, 全局边界与错误位置.

use ksl_base::name::Name;

use super::*;
use crate::runtime::scheduler::Scheduler;

fn run_on(
    session: &mut ksl_frontend::check::Session,
    scheduler: &mut Scheduler,
    source: &str,
) -> Result<Value, Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    crate::testutil::run_program(scheduler, session, &program)
}

fn run_on_int(
    session: &mut ksl_frontend::check::Session,
    scheduler: &mut Scheduler,
    source: &str,
) -> i64 {
    match run_on(session, scheduler, source) {
        Ok(Value::I64(value)) => value,
        other => panic!("结果不是整数: {other:?}"),
    }
}

#[test]
fn bindings_follow_scope_across_inputs() {
    let mut session = ksl_frontend::check::Session::new();
    let mut scheduler = Scheduler::new();
    assert_eq!(
        run_on_int(
            &mut session,
            &mut scheduler,
            "@Let[x, 1]; @Block[@Let[y, 2], AddI64[x, 1]]"
        ),
        2
    );
    assert!(!scheduler.globals().contains_key(&Name::new("y")));
    assert_eq!(run_on_int(&mut session, &mut scheduler, "x"), 1);
}

#[test]
fn runtime_error_span_points_at_expression() {
    let source = "@Let[x, DivideI64[1, 0]];";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = ksl_frontend::check::Session::new();
    let mut scheduler = Scheduler::new();
    let Error::Runtime(error) =
        crate::testutil::run_program(&mut scheduler, &mut session, &program)
            .expect_err("应报运行时错误")
    else {
        panic!("应为运行时错误");
    };
    let span = error.span().expect("运行时错误应带位置");
    assert_eq!(&source[span.start()..span.end()], "DivideI64[1, 0]");
}

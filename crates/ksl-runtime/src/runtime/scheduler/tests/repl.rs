//! 多段输入复用: 旧定义在后续输入中保持原语义.

use ksl_frontend::check::Session;
use ksl_value::value::Value;

use super::run_on;
use crate::runtime::scheduler::Scheduler;

#[test]
fn repl_keeps_previous_definitions() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    let first = r"
@Def[f, [n : i64] : i64, AddI64[n, 1]];
";
    run_on(&mut scheduler, &mut session, first).expect("定义应可执行");

    let second = r"
@Def[g, [n : i64] : i64, MulI64[f[n], f[n]]];
g[20]
";
    let value = run_on(&mut scheduler, &mut session, second).expect("调用应可执行");
    assert!(matches!(value, Value::I64(441)), "旧定义应可调用: {value:?}");

    let third = r"
@Def[f, [n : i64] : i64, AddI64[n, 2]];
@Let[new_f, f[20]];
@Let[old_g, g[20]];
AddI64[MulI64[new_f, 1000], old_g]
";
    let value = run_on(&mut scheduler, &mut session, third).expect("调用应可执行");
    scheduler.shutdown();
    assert!(
        matches!(value, Value::I64(22441)),
        "新 f 应为 22, 旧 g 应保持 441: {value:?}"
    );
}

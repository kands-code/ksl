//! `cps::gc` 模块测试.

use std::sync::Arc;

use crate::check::Session;
use crate::cps::scheduler::Scheduler;
use crate::error::RuntimeError;
use crate::value::{ActorHandle, Value};

fn checked(session: &mut Session, source: &str) -> crate::ast::Program {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    program
}

fn run(session: &mut Session, scheduler: &mut Scheduler, source: &str) -> Value {
    let program = checked(session, source);
    scheduler
        .run_program(&program, session.types())
        .expect("脚本应可运行")
}

fn find_pid(value: &Value) -> Option<Arc<ActorHandle>> {
    match value {
        Value::Pid(pid) => Some(Arc::clone(pid)),
        Value::Data(data) => data.fields.iter().find_map(find_pid),
        Value::Pair(pair) => find_pid(&pair.0).or_else(|| find_pid(&pair.1)),
        Value::List(list) => match &**list {
            crate::value::List::Values(items) => items.iter().find_map(find_pid),
            crate::value::List::I64(_)
            | crate::value::List::U8(_)
            | crate::value::List::U64(_)
            | crate::value::List::F64(_)
            | crate::value::List::Char(_)
            | crate::value::List::Bool(_) => None,
        },
        _ => None,
    }
}

#[test]
fn recursive_closures_follow_reachability() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    // 全局递归函数是根, 收集后仍可调用.
    let value = run(
        &mut session,
        &mut scheduler,
        r"
        @Def[count, [n : i64] : i64,
            @If[EqualI64[n, 0], 0, count[SubI64[n, 1]]]];
        count
        ",
    );
    let Value::Closure(closure) = &value else {
        panic!("结果应为闭包");
    };
    let reachable = Arc::downgrade(closure);
    drop(value);

    scheduler.collect_cycles(&[]);
    assert!(reachable.upgrade().is_some(), "全局递归函数不应被回收");
    assert!(matches!(
        run(&mut session, &mut scheduler, "count[3]"),
        Value::I64(0)
    ));

    // 重定义后旧闭包只剩自引用环, 不再被全局名字引用.
    let value = run(
        &mut session,
        &mut scheduler,
        r"
        @Def[unused, [n : i64] : i64,
            @If[EqualI64[n, 0], 0, unused[SubI64[n, 1]]]];
        unused
        ",
    );
    let Value::Closure(closure) = &value else {
        panic!("结果应为闭包");
    };
    let unreachable = Arc::downgrade(closure);
    drop(value);
    run(
        &mut session,
        &mut scheduler,
        r"
        @Def[unused, [n : i64] : i64, n];
        nil
        ",
    );
    scheduler.collect_cycles(&[]);

    assert!(unreachable.upgrade().is_none(), "不可达递归闭包应被回收");
    scheduler.shutdown();
}

#[test]
fn actor_result_cycle_is_collected() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    let value = run(
        &mut session,
        &mut scheduler,
        r"
        @Data[D, {C[Pid[D, D]]}];
        @Block[
            @Let[w, @Handler[[self : Pid[D, D]] : D,
                @Block[
                    @Let[msg, @Receive[0u, Option/Some[1000U]]],
                    @Match[msg, {
                        (/Some[d], d),
                        (/None[], D/C[self])
                    }]
                ]]],
            @Block[
                @Let[p, @Spawn[1u, w]],
                @Let[s, @Send[p, 0u, D/C[p]]],
                @Match[@Join[p], {
                    (/Ok[r], r),
                    (/Err[e], D/C[p])
                }]
            ]
        ]
        ",
    );
    let pid = find_pid(&value).expect("结果应包含自身的 Pid");
    let weak = Arc::downgrade(&pid);
    drop(pid);
    drop(value);

    // 句柄只剩结果中的 Pid 自引用.
    scheduler.collect_cycles(&[]);

    assert!(weak.upgrade().is_none(), "Actor 结果中的自引用环应被回收");
    scheduler.shutdown();
}

#[test]
fn automatic_collection_roots_rollback_snapshot() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    let value = run(
        &mut session,
        &mut scheduler,
        r"
        @Def[f, [n : i64] : i64,
            @If[EqualI64[n, 0], 0, f[SubI64[n, 1]]]];
        f
        ",
    );
    let Value::Closure(closure) = &value else {
        panic!("结果应为闭包");
    };
    let weak = Arc::downgrade(closure);
    drop(value);

    // 低阈值确保执行中途触发收集; 先替换全局 f, 再用长递归跨过预算边界.
    scheduler.set_gc_threshold(1);
    let before = scheduler.collection_count();
    let program = checked(
        &mut session,
        r"
        @Def[f, [n : i64] : i64,
            @If[EqualI64[n, 0], 0, f[SubI64[n, 1]]]];
        @Let[_, f[2000]];
        DivideI64[1, 0]
        ",
    );
    let error = scheduler
        .run_program(&program, session.types())
        .expect_err("除零应产生运行时错误");
    assert!(matches!(error, RuntimeError::Runtime(_)));
    assert!(
        scheduler.collection_count() >= before + 2,
        "执行中途应至少发生一次自动收集"
    );

    // 回滚后的旧闭包环境必须完整, 递归调用应继续可用.
    let result = run(&mut session, &mut scheduler, "f[3]");
    assert!(matches!(result, Value::I64(0)));
    assert!(weak.upgrade().is_some(), "全局与快照中的旧闭包不应被回收");
    scheduler.shutdown();
}

#[test]
fn suspended_actor_survives_collection() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    let value = run(
        &mut session,
        &mut scheduler,
        r"
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
            @Match[@Receive[0u, Option/None[]], {
                (/Some[m], m),
                (/None[], 0)
            }]]];
        @Let[p, @Spawn[1u, w]];
        nil
        ",
    );
    assert!(matches!(value, Value::Nil));

    scheduler.collect_cycles(&[]);

    let result = run(
        &mut session,
        &mut scheduler,
        r"
        @Let[s, @Send[p, 0u, 5]];
        @Match[@Join[p], {
            (/Ok[v], v),
            (/Err[e], 0)
        }]
        ",
    );
    assert!(matches!(result, Value::I64(5)), "挂起 Actor 应继续正常工作");
    scheduler.shutdown();
}

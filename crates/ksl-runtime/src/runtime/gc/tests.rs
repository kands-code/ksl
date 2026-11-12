//! `runtime::gc` 模块测试.

use std::sync::Arc;

use ksl_diagnostics::error::RuntimeError;
use ksl_frontend::check::Session;
use ksl_value::value::{ActorHandle, Value};

use crate::runtime::scheduler::Scheduler;

fn checked(source: &str) -> ksl_frontend::ast::Program {
    crate::testutil::parse(source).expect("脚本应可解析")
}

fn run(session: &mut Session, scheduler: &mut Scheduler, source: &str) -> Value {
    let program = checked(source);
    crate::testutil::run_program(scheduler, session, &program).expect("脚本应可运行")
}

/// 执行一个返回 Handler 的剧本, 再以根 Actor 运行该 Handler.
fn run_handler(
    session: &mut Session,
    scheduler: &mut Scheduler,
    source: &str,
    args: Vec<Value>,
) -> Value {
    let program = checked(source);
    let checked = session.check_typed(&program, None).expect("检查应成功");
    let tail = checked.tail.as_ref().expect("剧本应有尾表达式");
    let Some((pid, _)) = tail.ty.as_handler() else {
        panic!("尾表达式应是 Handler");
    };
    let Some((mailboxes, _)) = pid.as_pid() else {
        panic!("Handler 的 self 应是 Pid");
    };
    let handler = scheduler
        .run_linear(ksl_ir::compile(&checked))
        .expect("Handler 蓝图应可求值");
    scheduler
        .run_main_with_args(handler, mailboxes.len(), args)
        .expect("Handler 应可运行")
}

fn find_pid(value: &Value) -> Option<Arc<ActorHandle>> {
    match value {
        Value::Pid(pid) => Some(Arc::clone(pid)),
        Value::Data(data) => data.fields.iter().find_map(find_pid),
        Value::Pair(pair) => find_pid(&pair.0).or_else(|| find_pid(&pair.1)),
        Value::List(list) => match &**list {
            ksl_value::value::List::Values(items) => items.iter().find_map(find_pid),
            ksl_value::value::List::I64(_)
            | ksl_value::value::List::I32(_)
            | ksl_value::value::List::U8(_)
            | ksl_value::value::List::U32(_)
            | ksl_value::value::List::U64(_)
            | ksl_value::value::List::F32(_)
            | ksl_value::value::List::F64(_)
            | ksl_value::value::List::Char(_)
            | ksl_value::value::List::Bool(_) => None,
        },
        _ => None,
    }
}

#[test]
fn reachable_recursive_closure_survives_collection() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

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
    scheduler.shutdown();
}

#[test]
fn unreachable_recursive_closure_is_collected() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

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

    let value = run_handler(
        &mut session,
        &mut scheduler,
        r"
@Data[D, {C[Pid[D, D]]}];
@Let[worker, @Handler[[self : Pid[D, D]] : D,
  @Block[
    @Let[msg, @Receive[0u8, Option/Some[1000u64]]],
    @Match[msg, {
      (/Some[d], d),
      (/None[], D/C[self])
    }]
  ]]];
@Let[main, @Handler[[self : Pid[nil, D]] : D,
  @Block[
    @Let[p, @Spawn[worker]],
    @Let[s, @Send[p, 0u8, D/C[p]]],
    @Match[@Join[p], {
      (/Ok[r], r),
      (/Err[e], D/C[p])
    }]
  ]]];
main
",
        Vec::new(),
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
fn rollback_snapshot_values_survive_automatic_collection() {
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

    // 低阈值使执行中途触发收集; 先替换全局 f, 再用长递归跨过预算边界.
    scheduler.set_gc_threshold(1);
    let before = scheduler.collection_count();
    let program = checked(
        r"
@Def[f, [n : i64] : i64,
  @If[EqualI64[n, 0], 0, f[SubI64[n, 1]]]];
@Let[_, f[2000]];
DivideI64[1, 0]
",
    );
    let error = crate::testutil::run_program(&mut scheduler, &mut session, &program)
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

    let worker = run_handler(
        &mut session,
        &mut scheduler,
        r"
@Let[w, @Handler[[self : Pid[i64, i64]] : i64,
  @Match[@Receive[0u8, Option/None[]], {
    (/Some[m], m),
    (/None[], 0)
  }]]];
@Let[starter, @Handler[[self : Pid[nil, Pid[i64, i64]]] : Pid[i64, i64],
  @Block[
    @Let[p, @SpawnDetached[w]],
    p
  ]]];
starter
",
        Vec::new(),
    );
    assert!(matches!(worker, Value::Pid(_)));

    scheduler.collect_cycles(std::slice::from_ref(&worker));

    let result = run_handler(
        &mut session,
        &mut scheduler,
        r"
@Let[consumer, @Handler[[self : Pid[nil, i64], p : Pid[i64, i64]] : i64,
  @Block[
    @Let[s, @Send[p, 0u8, 5]],
    @Match[@Join[p], {
      (/Ok[v], v),
      (/Err[e], 0)
    }]
  ]]];
consumer
",
        vec![worker],
    );
    assert!(matches!(result, Value::I64(5)), "挂起 Actor 应继续正常工作");
    scheduler.shutdown();
}

#[test]
fn shared_containers_are_marked_once() {
    use ksl_value::value::{Key, List};

    let mut shared = Value::List(Arc::new(List::from_values([Value::I64(1)])));
    for _ in 0..8 {
        shared = Value::List(Arc::new(List::from_values([shared.clone(), shared])));
    }
    let dict: imbl::OrdMap<Key, Value> = [(Key::I64(1), Value::I64(2))].into_iter().collect();
    let dict = Value::Dict(Arc::new(dict));

    let mut marker = super::Marker::new();
    marker.mark_value(&shared);
    marker.mark_value(&dict);
    marker.run();

    // 9 个 List 与 1 个 Dict 各自只展开一次.
    assert_eq!(marker.objects.len(), 10);
}

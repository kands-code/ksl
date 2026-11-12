//! Actor 生命周期与错误上报.

use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::Value;

use super::run_with;
use crate::runtime::scheduler::Scheduler;

#[test]
fn actor_shares_command_line_args() {
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[w, @Handler[[self : Pid[i64, u64]] : u64, @ListLength[Args[]]]],
    @Let[p, @Spawn[w]],
    @Let[v, @Match[@Join[p], { (/Ok[v], v), (/Err[e], 0u64) }]],
    @Assert[EqualU64[v, 2u64], "Args 在 Actor 内可见"]
  ]]
"#;
    let mut scheduler = Scheduler::new();
    scheduler.set_args(vec!["a".to_string(), "b".to_string()]);
    let result = run_with(&mut scheduler, source);
    scheduler.shutdown();
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
}

#[test]
fn send_timers_clean_up_when_target_exits() {
    let source = r"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
      @Match[@Receive[0u8, Option/None[]], {
        (/Some[n], n),
        (/None[], 0)
      }]]],
    @Let[p, @Spawn[w]],
    @Let[timer, @SetTimer[p, 0u8, 1, 1000u64]],
    @Let[killed, @Kill[p]],
    @Let[joined, @Join[p]]
  ]]
";
    let mut scheduler = Scheduler::new();
    let result = run_with(&mut scheduler, source);
    assert!(matches!(result, Ok(Value::Nil)));
    assert!(
        scheduler.coordinator.send_timers.is_empty(),
        "目标终止后应清理延迟消息"
    );
    scheduler.shutdown();
}

#[test]
fn deep_actor_tree_is_torn_down_iteratively() {
    let source = r"
@Def[chain, [] : Handler[Pid[i64, nil], Pid[i64, nil], i64],
  @Handler[[self : Pid[i64, nil], sink : Pid[i64, nil], depth : i64] : nil,
    @Block[
      @If[GreaterI64[depth, 0],
        @Block[
          @Let[_, @Spawn[chain[], self, SubI64[depth, 1]]],
          @Let[_, @Receive[0u8, Option/None[]]],
          nil
        ],
        nil],
      @Let[_, @Send[sink, 0u8, 0]],
      @Match[@Receive[0u8, Option/None[]], {
        (/Some[_], nil),
        (/None[], nil)
      }]
    ]]
  ];

@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[waiter, @Handler[[self : Pid[i64, nil]] : nil,
      @Match[@Receive[0u8, Option/None[]], {
        (/Some[_], nil),
        (/None[], nil)
      }]]],
    @Let[w, @Spawn[waiter]],
    @Let[_, @Spawn[chain[], w, DEPTH]],
    @Let[_, @Join[w]],
    nil
  ]]
"
    .replace("DEPTH", "20000");
    let mut scheduler = Scheduler::new();
    let result = run_with(&mut scheduler, &source);
    scheduler.shutdown();
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
}

#[test]
fn actor_failures_are_reported_only_when_unobserved() {
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[bad, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]],
    @Let[sleeper, @Handler[[self : Pid[i64, i64]] : i64,
      @Block[@Sleep[10u64], 0]]],
    @Let[_, @Spawn[bad]],
    @Let[s, @Spawn[sleeper]],
    @Let[v, @Match[@Join[s], { (/Ok[v], v), (/Err[e], -1) }]],
    @Assert[EqualI64[v, 0], "未观察的 Actor 失败不影响主流程"],
    @Let[_, @Join[@Spawn[bad]]]
  ]]
"#;
    let mut scheduler = Scheduler::new();
    let result = run_with(&mut scheduler, source);
    assert!(matches!(result, Ok(Value::Nil)));

    let errors = scheduler.take_actor_errors();
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        Error::Runtime(error)
            if matches!(error.kind(), Some(ksl_diagnostics::error::ErrorKind::DivisionByZero))
    ));
    assert!(scheduler.take_actor_errors().is_empty());
    scheduler.shutdown();
}

#[test]
fn cancelled_join_failure_is_reported() {
    let errors = run_errors(
        r"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[bad, @Handler[[self : Pid[i64, i64]] : i64,
      @Block[@Sleep[50u64], DivideI64[1, 0]]]],
    @Let[joiner, @Handler[[self : Pid[i64, i64], target : Pid[i64, i64]] : i64,
      @Match[@Join[target], { (/Ok[v], v), (/Err[_], -1) }]]],
    @Let[b, @Spawn[bad]],
    @Let[j, @Spawn[joiner, b]],
    @Let[_, @Sleep[20u64]],
    @Let[_, @Kill[j]],
    @Let[_, @Sleep[80u64]],
    nil
  ]]
",
    );
    assert_eq!(errors.len(), 1, "Join 等待者终止后失败应作为未观察错误报告");
    assert!(matches!(
        &errors[0],
        Error::Runtime(error)
            if matches!(error.kind(), Some(ksl_diagnostics::error::ErrorKind::DivisionByZero))
    ));
}

fn run_errors(source: &str) -> Vec<Error> {
    let mut scheduler = Scheduler::new();
    let result = run_with(&mut scheduler, source);
    assert!(matches!(result, Ok(Value::Nil)), "脚本应正常结束: {result:?}");
    let errors = scheduler.take_actor_errors();
    scheduler.shutdown();
    errors
}

//! 调度公平性: 长基本块必须让出给就绪 Actor.

use ksl_value::value::Value;

use super::run_with;
use crate::runtime::scheduler::Scheduler;

#[test]
fn instruction_budget_lets_ready_actor_run() {
    let items = (0..100).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let source = r#"
@Main[[self : Pid[i64, nil]] : nil,
  @Block[
    @Let[worker, @Handler[[self : Pid[nil, bool], parent : Pid[i64, nil]] : bool,
      @Send[parent, 0u8, 1]]],
    @Let[_, @Spawn[worker, self]],
    @Let[_, {ITEMS}],
    @Assert[
      @IsSome[@Receive[0u8, Option/Some[1000u64]]],
      "长基本块应让出给就绪 Actor"
    ]
  ]]
"#
    .replace("ITEMS", &items);
    let mut scheduler = Scheduler::new();
    scheduler.set_slice_budget(20);
    let result = run_with(&mut scheduler, &source);
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
    scheduler.shutdown();
}

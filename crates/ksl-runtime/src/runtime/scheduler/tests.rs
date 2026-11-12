//! `runtime::scheduler` 模块测试.
//!
//! 覆盖调度器内部状态, 宿主配置与生命周期路径.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ksl_frontend::check::Session;

use super::*;
use crate::testutil::TempDir;
use crate::value::ActorHandle;

fn run(source: &str) -> Result<Value, Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    scheduler.shutdown();
    result
}

fn run_int(source: &str) -> i64 {
    match run(source) {
        Ok(Value::I64(value)) => value,
        Ok(other) => panic!("结果不是整数: {other:?}"),
        Err(error) => panic!("求值失败: {error:?}"),
    }
}

fn run_text(source: &str) -> String {
    match run(source) {
        Ok(Value::Str(text)) => text.to_string(),
        Ok(other) => panic!("结果不是字符串: {other:?}"),
        Err(error) => panic!("求值失败: {error:?}"),
    }
}

fn run_ok(source: &str) {
    if let Err(error) = run(source) {
        panic!("求值失败: {error:?}");
    }
}

#[test]
fn actor_shares_command_line_args() {
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[w, @Handler[[self : Pid[i64, u64]] : u64, @ListLength[Args[]]]],
    @Let[p, @Spawn[w]],
    @Let[v, @Match[@Join[p], { (/Ok[v], v), (/Err[e], 0U) }]],
    @Assert[EqualU64[v, 2U], "Args 在 Actor 内可见"]
  ]]
"#;
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    scheduler.set_args(vec!["a".to_string(), "b".to_string()]);
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    scheduler.shutdown();
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
}

#[test]
fn join_failure_carries_error_title() {
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[w, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]],
    @Let[p, @Spawn[w]],
    @Let[message, @Match[@Join[p], {
      (/Ok[v], ""),
      (/Err[e], @Match[e, { (/Failed[c, m], m), (_, "") }])
    }]],
    @Assert[EqualString[message, TITLE], "运行时错误带标题"]
  ]]
"#
    .replace(
        "TITLE",
        &format!("\"{}\"", ksl_frontend::error::ErrorKind::DivisionByZero.title()),
    );
    run_ok(&source);
}

#[test]
fn handler_literals_keep_suffix() {
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[w, @Handler[[self : Pid[i64, u8]] : u8,
      @Block[@Let[n : u8, 1u], n]]],
    @Let[p, @Spawn[w]],
    @Let[v, @Match[@Join[p], { (/Ok[v], v), (/Err[e], 0u) }]],
    @Assert[EqualU8[v, 1u], "Handler 体内的 u8 后缀保留"]
  ]]
"#;
    run_ok(source);
}

#[test]
fn repl_functions_see_previous_globals() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    let first = r"
@Def[f, [n : i64] : i64, AddI64[n, 1]];
";
    let program = crate::testutil::parse(first).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    crate::testutil::run_program(&mut scheduler, &session, &program).expect("定义应可执行");

    let second = r"
@Def[g, [n : i64] : i64, MulI64[f[n], f[n]]];
g[20]
";
    let program = crate::testutil::parse(second).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    let value =
        crate::testutil::run_program(&mut scheduler, &session, &program).expect("调用应可执行");
    scheduler.shutdown();
    assert!(matches!(value, Value::I64(441)), "结果应为 441: {value:?}");
}

#[test]
fn repl_redefinition_keeps_old_closure() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    for source in [
        r"
@Def[f, [n : i64] : i64, AddI64[n, 1]];
",
        r"
@Def[g, [n : i64] : i64, MulI64[f[n], f[n]]];
",
    ] {
        let program = crate::testutil::parse(source).expect("脚本应可解析");
        session.check(&program).expect("脚本应通过类型检查");
        crate::testutil::run_program(&mut scheduler, &session, &program).expect("定义应可执行");
    }

    let third = r"
@Def[f, [n : i64] : i64, AddI64[n, 2]];
@Let[new_f, f[20]];
@Let[old_g, g[20]];
AddI64[MulI64[new_f, 1000], old_g]
";
    let program = crate::testutil::parse(third).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    let value =
        crate::testutil::run_program(&mut scheduler, &session, &program).expect("调用应可执行");
    scheduler.shutdown();
    assert!(
        matches!(value, Value::I64(22441)),
        "新 f 应为 22, 旧 g 应保持 441: {value:?}"
    );
}

#[test]
fn monitor_registry_cleans_up_when_watcher_exits() {
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[target, @Handler[[self : Pid[i64, i64]] : i64,
      @Match[@Receive[0u, Option/None[]], {
        (/Some[n], n),
        (/None[], 0)
      }]]],
    @Let[watcher, @Handler[
      [self : Pid[Pair[u64, ExitReason], i64], t : Pid[i64, i64]] : i64,
      @Block[
        @Let[m, @Monitor[t]],
        @Match[m, { (/Some[id], @Halt[]), (/None[], 0) }]
      ]]],
    @Let[t, @Spawn[target]],
    @Let[w, @Spawn[watcher, t]],
    @Let[v, @Match[@Join[w], { (/Ok[v], v), (/Err[e], -1) }]],
    @Assert[EqualI64[v, -1], "监视者退出后 Join 返回 Killed"]
  ]]
"#;
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    assert!(matches!(result, Ok(Value::Nil)));
    assert!(
        scheduler.coordinator.monitors_empty(),
        "监视登记应随监视者退出清理"
    );
    scheduler.shutdown();
}

#[test]
fn send_timers_clean_up_when_target_exits() {
    let source = r"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
      @Match[@Receive[0u, Option/None[]], {
        (/Some[n], n),
        (/None[], 0)
      }]]],
    @Let[p, @Spawn[w]],
    @Let[timer, @SetTimer[p, 0u, 1, 1000U]],
    @Let[killed, @Kill[p]],
    @Let[joined, @Join[p]]
  ]]
";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
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
          @Let[_, @Receive[0u, Option/None[]]],
          nil
        ],
        nil],
      @Let[_, @Send[sink, 0u, 0]],
      @Match[@Receive[0u, Option/None[]], {
        (/Some[_], nil),
        (/None[], nil)
      }]
    ]]
  ];

@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[waiter, @Handler[[self : Pid[i64, nil]] : nil,
      @Match[@Receive[0u, Option/None[]], {
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
    let program = crate::testutil::parse(&source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    scheduler.shutdown();
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
}

#[test]
fn unobserved_actor_failure_is_reported() {
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[bad, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]],
    @Let[sleeper, @Handler[[self : Pid[i64, i64]] : i64,
      @Block[@Sleep[10U], 0]]],
    @Let[b, @Spawn[bad]],
    @Let[s, @Spawn[sleeper]],
    @Let[v, @Match[@Join[s], { (/Ok[v], v), (/Err[e], -1) }]],
    @Assert[EqualI64[v, 0], "未观察的 Actor 失败不影响主流程"]
  ]]
"#;
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    assert!(matches!(result, Ok(Value::Nil)));

    let errors = scheduler.take_actor_errors();
    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0],
        Error::Runtime(error)
            if matches!(error.kind, ksl_frontend::error::ErrorKind::DivisionByZero)
    ));
    assert!(scheduler.take_actor_errors().is_empty());
    scheduler.shutdown();
}

#[test]
fn joined_actor_failure_is_not_reported() {
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[bad, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]],
    @Let[b, @Spawn[bad]],
    @Let[v, @Match[@Join[b], { (/Ok[v], v), (/Err[e], 0) }]],
    @Assert[EqualI64[v, 0], "已观察的失败返回 Join 结果"]
  ]]
"#;
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    assert!(matches!(result, Ok(Value::Nil)));
    assert!(scheduler.take_actor_errors().is_empty());
    scheduler.shutdown();
}

#[test]
fn inline_join_observes_failure() {
    let source = r"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[w, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]],
    @Let[_, @Join[@Spawn[w]]]
  ]]
";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    assert!(matches!(result, Ok(Value::Nil)));
    assert!(scheduler.take_actor_errors().is_empty());
    scheduler.shutdown();
}

#[test]
fn stream_file_round_trip() {
    let dir = TempDir::new("ksl_io_roundtrip_");
    let path = dir.path().join("stream.txt");
    let path = path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
@Match[OpenWrite["{path}"], {{
    (/Ok[w], @Block[
        @Let[_, WriteString[w, "a\nb\n"]],
        @Let[_, Close[w]],
        @Match[OpenRead["{path}"], {{
            (/Ok[r], @Block[
                @Let[l1, @ResultGet[ReadLineFrom[r], Option/None[]]],
                @Let[l2, @ResultGet[ReadLineFrom[r], Option/None[]]],
                @Let[_, Close[r]],
                @Let[a, @OptionGet[l1, ""]],
                @Let[b, @OptionGet[l2, ""]],
                f"{{a}}{{b}}"
            ]),
            (/Err[e], "")
        }}]
    ]),
    (/Err[e], "")
}}]
"#
    );
    assert_eq!(run_text(&source), "ab");
}

#[test]
fn chunked_read_stops_at_end() {
    let dir = TempDir::new("ksl_io_chunks_");
    let path = dir.path().join("chunks.bin");
    let path = path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
@Match[OpenWrite["{path}"], {{
    (/Ok[w], @Block[
        @Let[_, WriteBytes[w, {{65u, 66u, 10u}}]],
        @Let[_, Close[w]],
        @Match[OpenRead["{path}"], {{
            (/Ok[r], @Block[
                @Let[c1, @ResultGet[ReadChunk[r, 2U], Option/None[]]],
                @Let[c2, @ResultGet[ReadChunk[r, 2U], Option/None[]]],
                @Let[c3, @ResultGet[ReadChunk[r, 2U], Option/None[]]],
                @Let[_, Close[r]],
                @Let[n1, @ListLength[@OptionGet[c1, {{}}]]],
                @Let[n2, @ListLength[@OptionGet[c2, {{}}]]],
                @Let[n3, @ListLength[@OptionGet[c3, {{}}]]],
                AddI64[AddI64[U64ToI64[n1], U64ToI64[n2]], U64ToI64[n3]]
            ]),
            (/Err[e], -1)
        }}]
    ]),
    (/Err[e], -2)
}}]
"#
    );
    assert_eq!(run_int(&source), 3);
}

#[test]
fn large_chunk_request_reads_available_bytes() {
    #[cfg(not(target_pointer_width = "64"))]
    return;

    let dir = TempDir::new("ksl_io_large_chunk_");
    let path = dir.path().join("huge.bin");
    std::fs::write(&path, b"abc").expect("测试文件应可写入");
    let path = path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
@Match[OpenRead["{path}"], {{
    (/Ok[r], @Block[
        @Let[c, @ResultGet[ReadChunk[r, 1099511627776U], Option/None[]]],
        @Let[_, Close[r]],
        U64ToI64[@ListLength[@OptionGet[c, {{}}]]]
    ]),
    (/Err[e], -1)
}}]
"#
    );
    assert_eq!(run_int(&source), 3);
}

#[test]
fn instruction_budget_lets_ready_actor_run() {
    let items = (0..100).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let source = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[worker, @Handler[[self : Pid[i64, nil]] : nil,
      @Block[
        @Let[_, @Register["ready", self]],
        @Match[@Receive[0u, Option/None[]], {
          (/Some[_], nil),
          (/None[], nil)
        }]
      ]]],
    @Let[_, @Spawn[worker]],
    @Let[_, {ITEMS}],
    @Let[found : Option[Pid[i64, nil]], @WhereIs["ready"]],
    @Assert[@IsSome[found], "长基本块应让出给就绪 Actor"]
  ]]
"#
    .replace("ITEMS", &items);
    let program = crate::testutil::parse(&source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    scheduler.set_slice_budget(20);
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
    scheduler.shutdown();
}

#[test]
fn send_timer_heaps_drop_cancelled_entries() {
    let mut scheduler = Scheduler::new();
    let actor = Arc::new(ActorHandle::new(1, 1, None));
    let deadline = Instant::now() + Duration::from_secs(1);
    let id = scheduler
        .coordinator
        .set_timer(&actor, 0, Value::I64(1), deadline, None)
        .expect("延迟消息应可登记");
    assert!(scheduler.coordinator.next_send_deadline().is_some());
    assert!(scheduler.coordinator.cancel_timer(id, None));
    assert!(
        scheduler.coordinator.next_send_deadline().is_none(),
        "取消后堆内死条目应被跳过"
    );
    scheduler.shutdown();
}

#[test]
fn actor_io_resumes_after_completion() {
    let dir = TempDir::new("ksl_actor_io_");
    let path = dir.path().join("data.txt");
    let path = path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[written, WriteFile["{path}", "hello"]],
    @Match[written, {{
      (/Ok[n], @Block[
        @Let[reader, @Handler[
          [self : Pid[nil, Result[string, FileError]]] : Result[string, FileError],
          ReadFile["{path}"]]],
        @Let[p, @Spawn[reader]],
        @Let[text, @Match[@Join[p], {{
          (/Ok[result], @Match[result, {{
            (/Ok[text], text),
            (/Err[e], @Panic["读取失败"])
          }}]),
          (/Err[e], @Panic["读取 Actor 失败"])
        }}]],
        @Assert[EqualString[text, "hello"], "Actor I/O 应读到写入内容"]
      ]),
      (/Err[e], @Panic["写入失败"])
    }}]
  ]]
"#
    );
    let program = crate::testutil::parse(&source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
    scheduler.shutdown();
}

fn run_in(
    scheduler: &mut Scheduler,
    session: &mut Session,
    source: &str,
) -> Result<Value, Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    crate::testutil::run_program(scheduler, session, &program)
}

#[test]
fn detached_actors_survive_between_roots() {
    let mut scheduler = Scheduler::new();
    let mut session = Session::new();
    let first = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[worker, @Handler[[self : Pid[i64, i64]] : i64,
      @Match[@Receive[0u, Option/None[]], {
        (/Some[n], n),
        (/None[], 0)
      }]]],
    @Let[p, @SpawnDetached[worker]],
    @Let[_, @Register["survivor", p]]
  ]]
"#;
    let result = run_in(&mut scheduler, &mut session, first);
    assert!(matches!(result, Ok(Value::Nil)), "首个根应正常结束: {result:?}");

    let value = run_in(&mut scheduler, &mut session, "@Let[x, 1]; AddI64[x, 41]")
        .expect("Prelude 应可运行");
    assert!(
        matches!(value, Value::I64(42)),
        "Prelude 结果应为 42: {value:?}"
    );

    scheduler.collect_cycles(&[]);

    let second = r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[found : Option[Pid[i64, i64]], @WhereIs["survivor"]],
    @Assert[@IsSome[found], "Detached Actor 应仍已注册"],
    @Let[p, @Match[found, {
      (/Some[x], x),
      (/None[], @Panic["Detached Actor 已丢失"])
    }]],
    @Let[_, @Send[p, 0u, 9]],
    @Let[v, @Match[@Join[p], { (/Ok[v], v), (/Err[e], -1) }]],
    @Assert[EqualI64[v, 9], "跨根 Join 应返回值"]
  ]]
"#;
    let result = run_in(&mut scheduler, &mut session, second);
    assert!(
        matches!(result, Ok(Value::Nil)),
        "第二个根应正常结束: {result:?}"
    );
    scheduler.shutdown();
}

#[test]
fn deferred_releases_run_during_scheduling() {
    let source = r"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Sleep[5U],
    nil
  ]]
";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let job_ran = Arc::clone(&ran);
    scheduler.releases().push(Box::new(move || {
        job_ran.store(true, std::sync::atomic::Ordering::Relaxed);
    }));
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    assert!(result.is_ok(), "程序应正常结束: {result:?}");
    assert!(
        ran.load(std::sync::atomic::Ordering::Relaxed),
        "释放任务应在调度循环内执行"
    );
    scheduler.shutdown();
}

#[test]
fn delivered_monitor_suppresses_error() {
    let errors = run_errors(
        r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[bad, @Handler[[self : Pid[i64, i64]] : i64,
      @Block[@Sleep[20U], DivideI64[1, 0]]]],
    @Let[watcher, @Handler[
      [self : Pid[Pair[u64, ExitReason], i64], target : Pid[i64, i64]] : i64,
      @Block[
        @Let[id, @Match[@Monitor[target], { (/Some[id], id), (/None[], 0U) }]],
        @Match[@Receive[0u, Option/Some[100U]], { (/Some[_], 0), (/None[], -1) }]
      ]]],
    @Let[b, @Spawn[bad]],
    @Let[w, @Spawn[watcher, b]],
    @Let[v, @Match[@Join[w], { (/Ok[v], v), (/Err[_], -2) }]],
    @Assert[EqualI64[v, 0], "监视通知应已投递"]
  ]]
"#,
    );
    assert!(errors.is_empty(), "已投递的监视不应重复报告");
}

#[test]
fn unmonitored_failure_is_reported() {
    let errors = run_errors(
        r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[bad, @Handler[[self : Pid[i64, i64]] : i64,
      @Block[@Sleep[20U], DivideI64[1, 0]]]],
    @Let[watcher, @Handler[
      [self : Pid[Pair[u64, ExitReason], i64], target : Pid[i64, i64]] : i64,
      @Block[
        @Let[id, @Match[@Monitor[target], { (/Some[id], id), (/None[], 0U) }]],
        @Let[_, @Unmonitor[id]],
        @Let[_, @Sleep[30U]],
        @Match[@Receive[0u, Option/Some[10U]], { (/Some[_], 1), (/None[], 0) }]
      ]]],
    @Let[b, @Spawn[bad]],
    @Let[w, @Spawn[watcher, b]],
    @Let[v, @Match[@Join[w], { (/Ok[v], v), (/Err[_], -3) }]],
    @Assert[EqualI64[v, 0], "取消监视后不应收到通知"]
  ]]
"#,
    );
    assert_eq!(errors.len(), 1, "取消监视后的失败应作为未观察错误报告");
    assert!(matches!(
        &errors[0],
        Error::Runtime(error)
            if matches!(error.kind, ksl_frontend::error::ErrorKind::DivisionByZero)
    ));
}

#[test]
fn cancelled_join_failure_is_reported() {
    let errors = run_errors(
        r"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[bad, @Handler[[self : Pid[i64, i64]] : i64,
      @Block[@Sleep[50U], DivideI64[1, 0]]]],
    @Let[joiner, @Handler[[self : Pid[i64, i64], target : Pid[i64, i64]] : i64,
      @Match[@Join[target], { (/Ok[v], v), (/Err[_], -1) }]]],
    @Let[b, @Spawn[bad]],
    @Let[j, @Spawn[joiner, b]],
    @Let[_, @Sleep[20U]],
    @Let[_, @Kill[j]],
    @Let[_, @Sleep[80U]],
    nil
  ]]
",
    );
    assert_eq!(errors.len(), 1, "Join 等待者终止后失败应作为未观察错误报告");
    assert!(matches!(
        &errors[0],
        Error::Runtime(error)
            if matches!(error.kind, ksl_frontend::error::ErrorKind::DivisionByZero)
    ));
}

fn run_errors(source: &str) -> Vec<Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = crate::testutil::run_program(&mut scheduler, &session, &program);
    assert!(matches!(result, Ok(Value::Nil)), "脚本应正常结束: {result:?}");
    let errors = scheduler.take_actor_errors();
    scheduler.shutdown();
    errors
}

//! `cps::scheduler` 模块测试.
//!
//! 覆盖调度器内部状态, 宿主配置与脚本不便表达的生命周期路径;
//! 其余语言层功能由脚本用例覆盖.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::check::Session;
use crate::testutil::TempDir;
use crate::value::ActorHandle;

fn run(source: &str) -> Result<Value, Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = scheduler.run_program(&program, session.types());
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
    let result = scheduler.run_program(&program, session.types());
    scheduler.shutdown();
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
}

#[test]
fn kill_terminates_waiting_and_unstarted_actors() {
    // 等待消息的 Actor 被 Kill 后, Join 立即返回 Killed.
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
              @Block[
                @Let[msg, @Receive[0u, Option/None[]]],
                @Match[msg, { (/Some[m], m), (/None[], 0) }]
              ]]],
            @Let[p, @Spawn[w]],
            @Let[k, @Kill[p]],
            @Let[joined, @Match[@Join[p], { (/Ok[v], v), (/Err[e], -1) }]],
            @Assert[EqualI64[joined, -1], "等待中的 Actor 被 Kill 后 Join 返回 Killed"]
          ]]
    "#;
    run_ok(source);

    // 尚未开始执行就被 Kill 的 Actor 不会运行 Handler.
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
              @Match[@Receive[0u, Option/None[]], {
                (/Some[x], x),
                (/None[], 99)
              }]]],
            @Let[p, @Spawn[w]],
            @Let[k, @Kill[p]],
            @Let[joined, @Match[@Join[p], {
              (/Ok[v], 0),
              (/Err[e], @Match[e, { (/Killed[], 1), (_, 2) }])
            }]],
            @Assert[EqualI64[joined, 1], "未启动的 Actor 被 Kill 后不执行 Handler"]
          ]]
    "#;
    run_ok(source);
}

#[test]
fn join_reports_actor_exit_reasons() {
    // 正常 Halt 与运行时错误都成为 Join 的 Err.
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[w, @Handler[[self : Pid[i64, string]] : string, @Halt[]]],
            @Let[p, @Spawn[w]],
            @Let[v, @Match[@Join[p], { (/Ok[v], v), (/Err[e], "halt") }]],
            @Assert[EqualString[v, "halt"], "Halt 成为 Join 的 Err"]
          ]]
    "#;
    run_ok(source);

    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[w, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]],
            @Let[p, @Spawn[w]],
            @Let[c, @Match[@Join[p], {
              (/Ok[v], 0),
              (/Err[e], @Match[e, { (/Failed[c, m], c), (_, 0) }])
            }]],
            @Assert[EqualI64[c, DIVISION], "运行时错误带错误编号"]
          ]]
    "#
    .replace(
        "DIVISION",
        &crate::error::ErrorKind::DivisionByZero.number().to_string(),
    );
    run_ok(&source);

    // 自身 Join 是运行时错误, 不是死锁.
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
              @Match[@Join[self], { (/Ok[v], v), (/Err[e], 0) }]]],
            @Let[p, @Spawn[w]],
            @Let[c, @Match[@Join[p], {
              (/Ok[v], 0),
              (/Err[e], @Match[e, { (/Failed[c, m], c), (_, 0) }])
            }]],
            @Assert[EqualI64[c, JOIN_SELF], "自 Join 报运行时错误"]
          ]]
    "#
    .replace(
        "JOIN_SELF",
        &crate::error::ErrorKind::JoinSelf.number().to_string(),
    );
    run_ok(&source);
}

#[test]
fn parent_child_lifecycle() {
    // 父 Actor 退出会终止子 Actor.
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[child, @Handler[[self : Pid[i64, i64]] : i64,
              @Match[@Receive[0u, Option/None[]], {
                (/Some[x], x),
                (/None[], 0)
              }]]],

            @Let[reporter, @Handler[[self : Pid[Pid[i64, i64], string]] : string,
              @Block[
                @Let[m, @Receive[0u, Option/None[]]],
                @Match[m, {
                  (/Some[p], @Match[@Join[p], {
                    (/Ok[v], "ok"),
                    (/Err[e], @Match[e, {
                      (/Killed[], "killed"),
                      (_, "other")
                    }])
                  }]),
                  (/None[], "none")
                }]
              ]]],

            @Let[parent, @Handler[
              [self : Pid[i64, i64], r : Pid[Pid[i64, i64], string]] : i64,
              @Block[
                @Let[c, @Spawn[child]],
                @Let[s, @Send[r, 0u, c]],
                0
              ]]],

            @Let[r, @Spawn[reporter]],
            @Let[p, @Spawn[parent, r]],
            @Let[jp, @Join[p]],
            @Let[v, @Match[@Join[r], { (/Ok[v], v), (/Err[e], "err") }]],
            @Assert[EqualString[v, "killed"], "父退出终止子 Actor"]
          ]]
    "#;
    run_ok(source);

    // Detached 子 Actor 不受父 Actor 退出影响, 且可直接 Join.
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[child, @Handler[[self : Pid[i64, i64]] : i64,
              @Block[@Sleep[10U], 7]]],

            @Let[reporter, @Handler[[self : Pid[Pid[i64, i64], i64]] : i64,
              @Block[
                @Let[m, @Receive[0u, Option/None[]]],
                @Match[m, {
                  (/Some[p], @Match[@Join[p], {
                    (/Ok[v], v),
                    (/Err[e], -1)
                  }]),
                  (/None[], -2)
                }]
              ]]],

            @Let[parent, @Handler[[self : Pid[i64, i64], r : Pid[Pid[i64, i64], i64]] : i64,
              @Block[
                @Let[c, @SpawnDetached[child]],
                @Let[s, @Send[r, 0u, c]],
                0
              ]]],

            @Let[r, @Spawn[reporter]],
            @Let[p, @Spawn[parent, r]],
            @Let[jp, @Join[p]],
            @Let[v, @Match[@Join[r], { (/Ok[v], v), (/Err[e], -3) }]],
            @Assert[EqualI64[v, 7], "Detached 子 Actor 不受父退出影响"]
          ]]
    "#;
    run_ok(source);

    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[w, @Handler[[self : Pid[i64, i64], n : i64] : i64, n]],
            @Let[p, @SpawnDetached[w, 9]],
            @Let[v, @Match[@Join[p], { (/Ok[v], v), (/Err[e], 0) }]],
            @Assert[EqualI64[v, 9], "Detached Actor 可直接 Join"]
          ]]
    "#;
    run_ok(source);
}

#[test]
fn runtime_state_survives_handlers_and_repl_inputs() {
    // Handler 体内的 u8 字面量保留后缀, 返回的也是 u8.
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

    // 跨 REPL 输入调用函数时, 函数体中的 u8 后缀仍按 u8 解释.
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();
    let first = r"
        @Def[f, [n : u8] : u8,
          @Block[@Let[k : u8, 1u], AddU8[n, k]]];
    ";
    let program = crate::testutil::parse(first).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    scheduler
        .run_program(&program, session.types())
        .expect("定义应可执行");

    let second = r"
        @Let[x : u8, 41u];
        f[x]
    ";
    let program = crate::testutil::parse(second).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    let value = scheduler
        .run_program(&program, session.types())
        .expect("调用应可执行");
    scheduler.shutdown();
    match value {
        Value::U8(42) => {},
        other => panic!("结果应为 u8 42: {other:?}"),
    }

    // 新输入定义的函数可捕获上一输入留下的全局, 重新定义后旧闭包保持旧值.
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    let first = r"
        @Def[f, [n : i64] : i64, AddI64[n, 1]];
    ";
    let program = crate::testutil::parse(first).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    scheduler
        .run_program(&program, session.types())
        .expect("定义应可执行");

    // 第二个输入里的 g 只在外层看到上一输入留下的全局 f, 捕获必须穿透到 main.
    let second = r"
        @Def[g, [n : i64] : i64, MulI64[f[n], f[n]]];
        g[20]
    ";
    let program = crate::testutil::parse(second).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    let value = scheduler
        .run_program(&program, session.types())
        .expect("调用应可执行");
    assert!(matches!(value, Value::I64(441)), "结果应为 441: {value:?}");

    // 重新定义 f 后, 已定义的 g 仍持有定义时刻的旧值.
    let third = r"
        @Def[f, [n : i64] : i64, AddI64[n, 2]];
        @Let[new_f, f[20]];
        @Let[old_g, g[20]];
        AddI64[MulI64[new_f, 1000], old_g]
    ";
    let program = crate::testutil::parse(third).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    let value = scheduler
        .run_program(&program, session.types())
        .expect("调用应可执行");
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
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::Nil)));
    let shared = scheduler.shared.lock();
    assert!(shared.monitors.is_empty(), "监视登记应随监视者退出清理");
    assert!(shared.monitors_by_target.is_empty());
    assert!(shared.monitors_by_watcher.is_empty());
    drop(shared);
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
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::Nil)));
    let shared = scheduler.shared.lock();
    assert!(shared.send_timers.is_empty(), "目标终止后应清理延迟消息");
    drop(shared);
    scheduler.shutdown();
}

#[test]
fn actor_error_reporting_respects_observation() {
    // 未被 Join 的 Actor 失败会进入错误收集.
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
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::Nil)));

    let errors = scheduler.take_actor_errors();
    assert_eq!(errors.len(), 1);
    match &errors[0] {
        Error::Runtime(error) => {
            assert_eq!(
                error.kind.number(),
                crate::error::ErrorKind::DivisionByZero.number()
            );
        },
        other => panic!("应为运行时错误: {other:?}"),
    }
    assert!(scheduler.take_actor_errors().is_empty());
    scheduler.shutdown();

    // 已由 Join 观察的失败不应重复报告.
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
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::Nil)));
    assert!(
        scheduler.take_actor_errors().is_empty(),
        "已由 Join 观察的失败不应重复报告"
    );
    scheduler.shutdown();

    // 内联 @Join 同样观察失败, 即使没有绑定 pid.
    let source = r"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[w, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]],
            @Let[_joined, @Join[@Spawn[w]]]
          ]]
    ";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::Nil)));
    assert!(
        scheduler.take_actor_errors().is_empty(),
        "内联 Join 观察的失败不应重复报告"
    );
    scheduler.shutdown();
}

#[test]
fn file_io_round_trips() {
    let dir = TempDir::new("ksl_io_roundtrip_");
    let text_path = dir.path().join("text.txt");
    let text_path = text_path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
        @Let[w, WriteFile["{text_path}", "hello"]];
        @Match[w, {{
            (/Ok[n], @Match[ReadFile["{text_path}"], {{
                (/Ok[text], text),
                (/Err[e], "read-error")
            }}]),
            (/Err[e], "write-error")
        }}]
    "#
    );
    assert_eq!(run_text(&source), "hello");

    // 流式写入后逐行读回.
    let stream_path = dir.path().join("stream.txt");
    let stream_path = stream_path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
        @Match[OpenWrite["{stream_path}"], {{
            (/Ok[w], @Block[
                @Let[_a, WriteString[w, "a\nb\n"]],
                @Let[_b, Close[w]],
                @Match[OpenRead["{stream_path}"], {{
                    (/Ok[r], @Block[
                        @Let[l1, @ResultGet[ReadLineFrom[r], Option/None[]]],
                        @Let[l2, @ResultGet[ReadLineFrom[r], Option/None[]]],
                        @Let[_c, Close[r]],
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

    // 分段读取按实际内容返回, 末尾返回 None.
    let chunk_path = dir.path().join("chunks.bin");
    let chunk_path = chunk_path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
        @Match[OpenWrite["{chunk_path}"], {{
            (/Ok[w], @Block[
                @Let[_a, WriteBytes[w, {{65u, 66u, 10u}}]],
                @Let[_b, Close[w]],
                @Match[OpenRead["{chunk_path}"], {{
                    (/Ok[r], @Block[
                        @Let[c1, @ResultGet[ReadChunk[r, 2U], Option/None[]]],
                        @Let[c2, @ResultGet[ReadChunk[r, 2U], Option/None[]]],
                        @Let[c3, @ResultGet[ReadChunk[r, 2U], Option/None[]]],
                        @Let[_c, Close[r]],
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
fn file_io_edge_cases() {
    let dir = TempDir::new("ksl_io_edge_");

    // 按上限读取而非预分配: 1 TiB 的请求遇到 3 字节文件应立即返回.
    #[cfg(target_pointer_width = "64")]
    {
        let path = dir.path().join("huge.bin");
        std::fs::write(&path, b"abc").expect("测试文件应可写入");
        let path = path.to_str().expect("路径应为 UTF-8");
        let source = format!(
            r#"
            @Match[OpenRead["{path}"], {{
                (/Ok[r], @Block[
                    @Let[c, @ResultGet[ReadChunk[r, 1099511627776U], Option/None[]]],
                    @Let[_c, Close[r]],
                    U64ToI64[@ListLength[@OptionGet[c, {{}}]]]
                ]),
                (/Err[e], -1)
            }}]
        "#
        );
        assert_eq!(run_int(&source), 3);
    }

    // 缺失文件报告 IOError/NotFound.
    let missing = dir.path().join("missing.txt");
    let missing = missing.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
        @Match[ReadFile["{missing}"], {{
            (/Ok[text], "ok"),
            (/Err[e], @Match[e, {{
                (IOError/NotFound[p], "notfound"),
                (_, "other")
            }}])
        }}]
    "#
    );
    assert_eq!(run_text(&source), "notfound");

    // Actor 可以等待文件 I/O.
    let path = dir.path().join("actor.txt");
    let path = path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
        @Main[[self : Pid[nil, nil]] : nil,
            @Block[
                @Let[reader, @Handler[[self : Pid[i64, string]] : string,
                    @Match[ReadFile["{path}"], {{
                        (/Ok[text], text),
                        (/Err[e], "read-error")
                    }}]]],
                @Let[_w, @Match[WriteFile["{path}", "from-actor"], {{
                    (/Ok[n], n),
                    (/Err[e], nil)
                }}]],
                @Let[p, @Spawn[reader]],
                @Let[text, @Match[@Join[p], {{ (/Ok[text], text), (/Err[e], "err") }}]],
                @Assert[EqualString[text, "from-actor"], "Actor 等待文件 I/O"]
            ]]
    "#
    );
    run_ok(&source);
}

#[test]
fn instruction_budget_lets_ready_actor_run_before_long_block_finishes() {
    // 列表字面量会在线性 IR 中形成超长基本块; 若预算按基本块计数,
    // 主脚本会先完成 @WhereIs 检查, Actor 之后才有机会登记.
    let items = (0..100).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[worker, @Handler[[self : Pid[i64, nil]] : nil,
              @Block[
                @Let[_registered, @Register["ready", self]],
                @Match[@Receive[0u, Option/None[]], {
                  (/Some[_], nil),
                  (/None[], nil)
                }]
              ]]],
            @Let[_pid, @Spawn[worker]],
            @Let[_xs, {ITEMS}],
            @Let[found : Option[Pid[i64, nil]], @WhereIs["ready"]],
            @Assert[@IsSome[found], "长基本块应让出给就绪 Actor"]
          ]]
    "#
    .replace("ITEMS", &items);
    let program = crate::testutil::parse(&source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    // 让出公平性是协作式轮转的性质.
    let mut scheduler = Scheduler::new();
    // 缩小预算, 用较小的列表触发同一让出路径.
    scheduler.set_slice_budget(20);
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
    scheduler.shutdown();
}

#[test]
fn send_timer_heaps_drop_cancelled_entries() {
    // 延迟投递堆跳过已取消条目; 任务超时堆的失效逻辑由 TimerHeap 自身测试.
    let mut scheduler = Scheduler::new();
    let actor = Arc::new(ActorHandle::new(1, 1, None));
    let deadline = Instant::now() + Duration::from_secs(1);
    let id = scheduler
        .handle
        .set_timer(&actor, 0, Value::I64(1), deadline, None)
        .expect("延迟消息应可登记");
    assert!(scheduler.shared.lock().next_send_deadline().is_some());
    assert!(scheduler.handle.cancel_timer(id, None));
    assert!(
        scheduler.shared.lock().next_send_deadline().is_none(),
        "取消后堆内死条目应被跳过"
    );
    scheduler.shutdown();
}

fn run_in(
    scheduler: &mut Scheduler,
    session: &mut Session,
    source: &str,
) -> Result<Value, Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    scheduler.run_program(&program, session.types())
}

#[test]
fn actor_io_resumes_after_completion() {
    // 根 Actor 写入后, 子 Actor 读取; 两次 I/O 完成都需唤醒挂起的任务.
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
                  [self : Pid[nil, Result[string, IOError]]] : Result[string, IOError],
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
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
    scheduler.shutdown();
}

#[test]
fn stale_send_wake_does_not_break_supervisor_restart() {
    // 监督者在一轮内可能先消费消息再挂起等待下一轮; 上一轮排队的 Send
    // 唤醒事件不能把下一轮的 Receive 误唤醒为 None.
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[worker, @Handler[[self : Pid[i64, i64], n : i64] : i64, n]],
            @Let[supervisor, @Handler[
              [self : Pid[Pair[u64, ExitReason], i64], attempt : i64] : i64,
              @Block[
                @Let[child, @Spawn[worker, attempt]],
                @Let[_mon, @Monitor[child]],
                @Match[@Receive[0u, Option/None[]], {
                  (/Some[notice], @Match[@Snd[notice], {
                    (/Halted[], @If[EqualI64[attempt, 3],
                       attempt,
                       @Continue[AddI64[attempt, 1]]]),
                    (_, -1)
                  }]),
                  (/None[], -2)
                }]
              ]]],
            @Let[p, @Spawn[supervisor, 1]],
            @Let[v, @Match[@Join[p], { (/Ok[v], v), (/Err[e], -3) }]],
            @Assert[EqualI64[v, 3], "监督重启不应被过期唤醒打断"]
          ]]
    "#;
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::Nil)), "监督重启应完成: {result:?}");
    scheduler.shutdown();
}

#[test]
fn detached_actors_survive_between_roots() {
    // Detached Actor 在根结束后继续存活, 跨根 Send/Join 仍可达.
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
            @Let[_registered, @Register["survivor", p]]
          ]]
    "#;
    let result = run_in(&mut scheduler, &mut session, first);
    assert!(matches!(result, Ok(Value::Nil)), "首个根应正常结束: {result:?}");

    // 这里同时验证 Prelude 与存活 Actor 共存.
    let value = run_in(&mut scheduler, &mut session, "@Let[x, 1]; AddI64[x, 41]")
        .expect("Prelude 应可运行");
    assert!(
        matches!(value, Value::I64(42)),
        "Prelude 结果应为 42: {value:?}"
    );

    // 环回收不应清掉挂起 Actor.
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
            @Let[_s, @Send[p, 0u, 9]],
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

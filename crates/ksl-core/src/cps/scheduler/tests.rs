//! `cps::scheduler` 模块测试.

use super::*;
use crate::check::Session;
use crate::testutil::TempDir;

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

#[test]
fn actor_shares_command_line_args() {
    let source = r"
        @Let[w, @Handler[[self : Pid[i64, u64]] : u64, @ListLength[Args[]]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Match[@Join[p], { (/Ok[v], v), (/Err[e], 0U) }]
        ]
    ";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    scheduler.set_args(vec!["a".to_string(), "b".to_string()]);
    let result = scheduler.run_program(&program, session.types());
    scheduler.shutdown();
    assert!(matches!(result, Ok(Value::U64(2))));
}

#[test]
fn kill_terminates_waiting_and_unstarted_actors() {
    // 等待消息的 Actor 被 Kill 后, Join 立即返回 Killed.
    let source = r"
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
            @Block[
                @Let[msg, @Receive[0u, Option/None[]]],
                @Match[msg, { (/Some[m], m), (/None[], 0) }]
            ]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Let[k, @Kill[p]],
            @Match[@Join[p], { (/Ok[v], v), (/Err[e], -1) }]
        ]
    ";
    assert_eq!(run_int(source), -1);

    // 尚未开始执行就被 Kill 的 Actor 不会运行 Handler.
    let source = r"
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
            @Match[@Receive[0u, Option/None[]], {
                (/Some[x], x),
                (/None[], 99)
            }]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Let[k, @Kill[p]],
            @Match[@Join[p], {
                (/Ok[v], 0),
                (/Err[e], @Match[e, { (/Killed[], 1), (_, 2) }])
            }]
        ]
    ";
    assert_eq!(run_int(source), 1);
}

#[test]
fn join_reports_actor_exit_reasons() {
    // 正常 Halt 与运行时错误都成为 Join 的 Err.
    let source = r#"
        @Let[w, @Handler[[self : Pid[i64, string]] : string, @Halt[]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Match[@Join[p], { (/Ok[v], v), (/Err[e], "halt") }]
        ]
    "#;
    assert_eq!(run_text(source), "halt");

    let source = r"
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Match[@Join[p], {
                (/Ok[v], 0),
                (/Err[e], @Match[e, { (/Failed[c, m], c), (_, 0) }])
            }]
        ]
    ";
    assert_eq!(
        run_int(source),
        i64::from(crate::error::ErrorKind::DivisionByZero.number())
    );

    // 自身 Join 是运行时错误, 不是死锁.
    let source = r"
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
            @Match[@Join[self], { (/Ok[v], v), (/Err[e], 0) }]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Match[@Join[p], {
                (/Ok[v], 0),
                (/Err[e], @Match[e, { (/Failed[c, m], c), (_, 0) }])
            }]
        ]
    ";
    assert_eq!(
        run_int(source),
        i64::from(crate::error::ErrorKind::JoinSelf.number())
    );
}

#[test]
fn parent_child_lifecycle() {
    // 父 Actor 退出会终止子 Actor.
    let source = r#"
        @Let[child, @Handler[[self : Pid[i64, i64]] : i64,
            @Match[@Receive[0u, Option/None[]], {
                (/Some[x], x),
                (/None[], 0)
            }]]];

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
            ]]];

        @Let[parent, @Handler[[self : Pid[i64, i64], r : Pid[Pid[i64, i64], string]] : i64,
            @Block[
                @Let[c, @Spawn[1u, child]],
                @Let[s, @Send[r, 0u, c]],
                0
            ]]];

        @Block[
            @Let[r, @Spawn[1u, reporter]],
            @Let[p, @Spawn[1u, parent, r]],
            @Let[jp, @Join[p]],
            @Match[@Join[r], { (/Ok[v], v), (/Err[e], "err") }]
        ]
    "#;
    assert_eq!(run_text(source), "killed");

    // Detached 子 Actor 不受父 Actor 退出影响, 且可直接 Join.
    let source = r"
        @Let[child, @Handler[[self : Pid[i64, i64]] : i64,
            @Block[@Sleep[10U], 7]]];

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
            ]]];

        @Let[parent, @Handler[[self : Pid[i64, i64], r : Pid[Pid[i64, i64], i64]] : i64,
            @Block[
                @Let[c, @SpawnDetached[1u, child]],
                @Let[s, @Send[r, 0u, c]],
                0
            ]]];

        @Block[
            @Let[r, @Spawn[1u, reporter]],
            @Let[p, @Spawn[1u, parent, r]],
            @Let[jp, @Join[p]],
            @Match[@Join[r], { (/Ok[v], v), (/Err[e], -3) }]
        ]
    ";
    assert_eq!(run_int(source), 7);

    let source = r"
        @Let[w, @Handler[[self : Pid[i64, i64], n : i64] : i64, n]];
        @Block[
            @Let[p, @SpawnDetached[1u, w, 9]],
            @Match[@Join[p], { (/Ok[v], v), (/Err[e], 0) }]
        ]
    ";
    assert_eq!(run_int(source), 9);
}

#[test]
fn runtime_state_survives_handlers_and_repl_inputs() {
    // Handler 体内的 u8 字面量保留后缀, 返回的也是 u8.
    let source = r"
        @Let[w, @Handler[[self : Pid[i64, u8]] : u8,
            @Block[@Let[n : u8, 1u], n]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Match[@Join[p], { (/Ok[v], v), (/Err[e], 0u) }]
        ]
    ";
    match run(source) {
        Ok(Value::U8(1)) => {},
        other => panic!("结果应为 u8 1: {other:?}"),
    }

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
    let source = r"
        @Let[target, @Handler[[self : Pid[i64, i64]] : i64,
            @Match[@Receive[0u, Option/None[]], {
                (/Some[n], n),
                (/None[], 0)
            }]]];
        @Let[watcher, @Handler[
            [self : Pid[Pair[u64, ExitReason], i64], t : Pid[i64, i64]] : i64,
            @Block[
                @Let[m, @Monitor[t]],
                @Match[m, { (/Some[id], @Halt[]), (/None[], 0) }]
            ]]];
        @Block[
            @Let[t, @Spawn[1u, target]],
            @Let[w, @Spawn[1u, watcher, t]],
            @Match[@Join[w], { (/Ok[v], v), (/Err[e], -1) }]
        ]
    ";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::I64(-1))));
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
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
            @Match[@Receive[0u, Option/None[]], {
                (/Some[n], n),
                (/None[], 0)
            }]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Let[timer, @SetTimer[p, 0u, 1, 1000U]],
            @Let[killed, @Kill[p]],
            @Let[joined, @Join[p]],
            nil
        ]
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
fn failed_input_kills_only_actors_spawned_in_that_input() {
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();

    let first = r"
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
            @Match[@Receive[0u, Option/None[]], {
                (/Some[x], x),
                (/None[], 0)
            }]]];
        @Let[keep, @Spawn[1u, w]];
        keep
    ";
    let program = crate::testutil::parse(first).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    let value = scheduler
        .run_program(&program, session.types())
        .expect("启动应成功");
    let Value::Pid(keep) = value else {
        panic!("结果应为 Pid");
    };

    let second = r"
        @Let[p, @SpawnDetached[1u, w]];
        DivideI64[1, 0]
    ";
    let program = crate::testutil::parse(second).expect("脚本应可解析");
    session.check(&program).expect("脚本应通过类型检查");
    assert!(scheduler.run_program(&program, session.types()).is_err());

    assert_eq!(scheduler.actors.len(), 1);
    assert!(scheduler.actors.contains_key(&keep.id));
    scheduler.shutdown();
}

#[test]
fn actor_error_reporting_respects_observation() {
    // 未被 Join 的 Actor 失败会进入错误收集.
    let source = r"
        @Let[bad, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]];
        @Let[sleeper, @Handler[[self : Pid[i64, i64]] : i64,
            @Block[@Sleep[10U], 0]]];
        @Block[
            @Let[b, @Spawn[1u, bad]],
            @Let[s, @Spawn[1u, sleeper]],
            @Match[@Join[s], { (/Ok[v], v), (/Err[e], -1) }]
        ]
    ";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::I64(0))));

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
    let source = r"
        @Let[bad, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]];
        @Let[b, @Spawn[1u, bad]];
        @Match[@Join[b], { (/Ok[v], v), (/Err[e], 0) }]
    ";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    let result = scheduler.run_program(&program, session.types());
    assert!(matches!(result, Ok(Value::I64(0))));
    assert!(
        scheduler.take_actor_errors().is_empty(),
        "已由 Join 观察的失败不应重复报告"
    );
    scheduler.shutdown();

    // 内联 @Join 同样观察失败, 即使没有绑定 pid.
    let source = r"
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64, DivideI64[1, 0]]];
        @Let[_joined, @Join[@Spawn[1u, w]]];
        nil
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
fn deadlock_and_sleep() {
    // 所有任务都在等待时报告死锁.
    let source = r"
        @Let[w, @Handler[[self : Pid[i64, i64]] : i64,
            @Match[@Receive[0u, Option/None[]], {
                (/Some[x], x),
                (/None[], 0)
            }]]];
        @Block[
            @Let[p, @Spawn[1u, w]],
            @Join[p]
        ]
    ";
    assert!(matches!(
        run(source),
        Err(Error::Runtime(error)) if error.kind == ErrorKind::Deadlock
    ));

    // Sleep 挂起 Actor, 唤醒后继续执行.
    let source = r"
        @Let[w, @Handler[[self : Pid[i64, i64], n : i64] : i64,
            @Block[@Sleep[10U], n]]];
        @Block[
            @Let[p, @Spawn[1u, w, 7]],
            @Match[@Join[p], { (/Ok[v], v), (/Err[e], 0) }]
        ]
    ";
    assert_eq!(run_int(source), 7);
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
        @Let[reader, @Handler[[self : Pid[i64, string]] : string,
            @Match[ReadFile["{path}"], {{
                (/Ok[text], text),
                (/Err[e], "read-error")
            }}]]];
        @Block[
            @Match[WriteFile["{path}", "from-actor"], {{
                (/Ok[n], @Block[
                    @Let[p, @Spawn[1u, reader]],
                    @Match[@Join[p], {{ (/Ok[text], text), (/Err[e], "err") }}]
                ]),
                (/Err[e], "write-error")
            }}]
        ]
    "#
    );
    assert_eq!(run_text(&source), "from-actor");
}

#[test]
fn instruction_budget_lets_ready_actor_run_before_long_block_finishes() {
    // 列表字面量会在线性 IR 中形成超长基本块; 若预算按基本块计数,
    // 主脚本会先完成 @WhereIs 检查, Actor 之后才有机会登记.
    let items = (0..100).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let source = r#"
        @Let[worker, @Handler[[self : Pid[i64, nil]] : nil,
            @Block[
                @Let[_registered, @Register["ready", self]],
                @Match[@Receive[0u, Option/None[]], {
                    (/Some[_], nil),
                    (/None[], nil)
                }]
            ]]];
        @Let[_pid, @Spawn[1u, worker]];
        @Let[_xs, {ITEMS}];
        @Let[found : Option[Pid[i64, nil]], @WhereIs["ready"]];
        @IsSome[found]
    "#
    .replace("ITEMS", &items);
    let program = crate::testutil::parse(&source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let mut scheduler = Scheduler::new();
    // 缩小预算, 用较小的列表触发同一让出路径.
    scheduler.set_slice_budget(20);
    let result = scheduler.run_program(&program, session.types());
    assert!(
        matches!(result, Ok(Value::Bool(true))),
        "长基本块应让出给就绪 Actor: {result:?}"
    );
    scheduler.shutdown();
}

#[test]
fn timer_heaps_drop_cancelled_entries() {
    let mut scheduler = Scheduler::new();
    let span = Span::new(crate::source::SourceId::default(), 0, 0);
    scheduler
        .add_timer(1000, TaskId::Actor(1), TimerKind::Sleep, span)
        .expect("定时器应可登记");
    scheduler
        .add_timer(1000, TaskId::Actor(2), TimerKind::Receive, span)
        .expect("定时器应可登记");

    scheduler.clear_wait(TaskId::Actor(1));
    scheduler.prune_stale_timers();
    assert_eq!(scheduler.timers.len(), 1, "已取消的定时器应被清理");
    assert!(
        !scheduler.active_timers.contains_key(&TaskId::Actor(1)),
        "取消后不应保留有效编号"
    );
    assert!(
        scheduler.active_timers.contains_key(&TaskId::Actor(2)),
        "其他任务的定时器不应受影响"
    );

    // 延迟投递堆同样跳过已取消条目.
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

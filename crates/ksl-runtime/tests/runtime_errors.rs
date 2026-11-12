//! 运行时错误的种类断言.
//!
//! 错误编号由枚举顺序生成, 不是稳定契约; 本文件按 `ErrorKind` 断言错误语义.

use common::{parse, run_program};
use ksl_diagnostics::error::{ErrorKind, RuntimeError};
use ksl_frontend::check::Session;
use ksl_runtime::Scheduler;

mod common;

fn run_error(source: &str) -> ErrorKind {
    let program = parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    let mut scheduler = Scheduler::new();
    let result = run_program(&mut scheduler, &mut session, &program);
    scheduler.shutdown();
    match result {
        Err(RuntimeError::Runtime(error)) => {
            error.kind().cloned().expect("运行时错误应来自错误目录")
        },
        other => panic!("期望运行时错误, 实际 {other:?}"),
    }
}

macro_rules! assert_error {
    ($source:expr, $pattern:pat) => {{
        let kind = run_error($source);
        assert!(
            matches!(kind, $pattern),
            "错误种类不符: {kind:?}\n源码: {}",
            $source
        );
    }};
}

#[test]
fn fatal_primitives_report_their_kind() {
    assert_error!(
        r#"@Assert[LessI64[2, 1], "条件不成立"];"#,
        ErrorKind::AssertionFailed { .. }
    );
    assert_error!(r#"@Panic["出大事了"];"#, ErrorKind::Panicked { .. });
    assert_error!(r#"@Todo["尚未实现"];"#, ErrorKind::Unimplemented { .. });
    assert_error!(
        r#"@Unreachable["不该到达"];"#,
        ErrorKind::UnreachableReached { .. }
    );
}

#[test]
fn arithmetic_errors_report_their_kind() {
    assert_error!(
        "@Let[_, AddI64[9223372036854775807, 1]];",
        ErrorKind::IntegerOverflow
    );
    assert_error!(
        "@Let[_, QuotientI64[-9223372036854775808, -1]];",
        ErrorKind::IntegerOverflow
    );
    assert_error!("@Let[_, AddU8[255u8, 1u8]];", ErrorKind::U8Overflow);
    assert_error!("@Let[_, SubU32[0u32, 1u32]];", ErrorKind::U32Overflow);
    assert_error!(
        "@Let[_, AddU64[18446744073709551615u64, 1u64]];",
        ErrorKind::U64Overflow
    );
    assert_error!("@Let[_, NegI32[-2147483648i32]];", ErrorKind::I32Overflow);
    assert_error!(
        "@Let[_, RoundF64[9223372036854775808.0]];",
        ErrorKind::FloatOutOfRange { .. }
    );
    assert_error!(
        "@Let[_, I32ToU32[-1i32]];",
        ErrorKind::IntConversionOutOfRange { .. }
    );
    assert_error!("@Let[_, U32ToChar[1114112u32]];", ErrorKind::InvalidCharScalar);
}

#[test]
fn container_errors_report_their_kind() {
    assert_error!("@Let[_, @ListGet[{1}, 5u64]];", ErrorKind::IndexOutOfRange);
    assert_error!(
        "@Let[empty : List[i64], {}]; @Let[_, @ListHead[empty]];",
        ErrorKind::EmptyListHead
    );
    assert_error!(
        "@Let[_, @ListRiffle[{1, 2}, 0, 0u64]];",
        ErrorKind::InvalidInterval
    );
    assert_error!(
        "@Let[_, @ListWindow[{1, 2}, 0u64]];",
        ErrorKind::InvalidWindowSize
    );
    assert_error!(
        "@Let[_, @ListChunk[{1, 2}, 0u64]];",
        ErrorKind::InvalidChunkSize
    );
}

#[test]
fn time_errors_report_their_kind() {
    assert_error!(
        "@Let[_, EpochMillisToDateTime[9223372036854775807]];",
        ErrorKind::DateTimeOutOfRange
    );
}

#[test]
fn scheduler_errors_report_their_kind() {
    assert_error!(
        "@Main[[self : Pid[nil, nil]] : nil, @Block[@Let[_, @Join[self]], nil]]",
        ErrorKind::JoinSelf
    );
    assert_error!(
        r"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[worker, @Handler[[self : Pid[i64, i64]] : i64,
      @Match[@Receive[0u8, Option/None[]], {
        (/Some[x], x),
        (/None[], 0)
      }]]],
    @Let[p, @Spawn[worker]],
    @Let[_, @Join[p]]
  ]]
",
        ErrorKind::Deadlock
    );
    assert_error!(
        "@Main[[self : Pid[nil, nil]] : nil, @Block[@Let[_, @Kill[self]]]]",
        ErrorKind::KillRoot
    );
}

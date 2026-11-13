//! `cps::linear` 模块测试.

use std::fmt::Write as _;

use super::*;
use crate::cps::ir::lower_program;

fn linear(source: &str) -> String {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = crate::check::Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let module = lower_program(&program, session.types());
    linearize(&module).to_string()
}

#[test]
fn tail_call_and_return_are_terminators() {
    let ir = linear(
        r"
        @Def[count, [n : i64] : i64,
            @If[LessOrEqualI64[n, 0], 0, count[SubI64[n, 1]]]];
        count[3]
    ",
    );
    assert!(ir.contains("fn count("));
    assert!(ir.contains("if "));
    assert!(ir.contains("tailcall "));
    assert!(ir.contains("return "));
    assert!(ir.contains("free [count]"), "递归函数应捕获自身: {ir}");
}

#[test]
fn call_result_uses_a_target_block() {
    let ir = linear(
        r"
        @Def[addOne, [n : i64] : i64, AddI64[n, 1]];
        @Let[x, addOne[2]];
        x
    ",
    );
    assert!(ir.contains("call addOne"), "缺少调用: {ir}");
    assert!(ir.contains(" -> b"), "缺少目标块: {ir}");
}

#[test]
fn match_and_continuation_blocks_are_explicit() {
    let ir = linear(
        r"
        @Let[x, @Match[Option/Some[1], {
            (/Some[n], n),
            (/None[], 0)
        }]];
        x
    ",
    );
    assert!(ir.contains("match "));
    assert!(ir.contains("jump b"));
    // 模式绑定成为目标块参数, 运行时没有续延表.
    assert!(ir.contains("Some[n : i64] -> b"), "模式参数缺失: {ir}");
}

#[test]
fn closure_captures_only_free_variables() {
    let ir = linear(
        r"
        @Let[x, 10];
        @Let[f, @Fn[[y : i64] : i64, AddI64[x, y]]];
        f[5]
    ",
    );
    assert!(ir.contains("free [x]"), "匿名函数应只捕获 x: {ir}");
    assert!(!ir.contains("free [x, y]"), "形参不应进入捕获: {ir}");
}

#[test]
fn user_function_named_main_does_not_collide_with_entry() {
    let ir = linear(
        r"
        @Def[f, [n : i64] : i64, AddI64[n, 1]];
        @Def[main, [n : i64] : i64, f[n]];
        main[1]
    ",
    );
    assert!(ir.contains("fn main("), "用户函数应保留源码名: {ir}");
    assert!(ir.contains("fn $main("), "入口函数应使用内部名: {ir}");
}

#[test]
fn long_chains_do_not_use_call_stack() {
    // 用超过常规规模的输入覆盖降级, 自由变量分析, 线性化与 CPS 释放的
    // 显式栈路径, 防止长链耗尽测试线程栈.
    let items = (0..2000).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let list_ir = linear(&format!("@Let[xs, {{{items}}}]; xs"));
    assert!(list_ir.contains("fn $main("), "长列表应完成线性化");

    let mut source = String::new();
    for i in 0..2000 {
        writeln!(source, "@Let[x{i}, AddI64[{i}, 0]];").expect("写入字符串应成功");
    }
    source.push_str("x1999");
    let seq_ir = linear(&source);
    assert!(seq_ir.contains("fn $main("), "长语句序列应完成线性化");
}

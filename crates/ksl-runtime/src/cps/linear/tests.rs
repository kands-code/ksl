//! `cps::linear` 模块测试.

use std::fmt::Write as _;

use super::*;
use crate::cps::ir::lower_program;

fn linear(source: &str) -> String {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = ksl_frontend::check::Session::new();
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
fn pure_builtin_calls_fuse_into_the_caller_block() {
    let ir = linear(
        r"
@Def[f, [n : i64] : i64, @Block[
  @Let[x, AddI64[n, 1]],
  x
]];
f[1]
",
    );
    assert!(ir.contains("let x#1 = @AddI64"), "纯内置调用应融合为指令: {ir}");
    assert!(!ir.contains("call @AddI64"), "不应保留调用终止指令: {ir}");
}

#[test]
fn shared_continuation_blocks_are_not_fused() {
    let ir = linear(
        r"
@Def[f, [n : i64] : i64, @Block[
  @Let[x, @Which[{(EqualI64[n, 0], AddI64[n, 1])}, AddI64[n, 2]]],
  x
]];
f[1]
",
    );
    // 两个分支共享的结果块保留为跳转目标; 各自调用仍融合为指令.
    assert!(ir.contains("jump b"), "共享续延应保留跳转: {ir}");
    assert!(!ir.contains("call @AddI64"), "调用本身应融合: {ir}");
}

#[test]
fn synchronous_primitives_fuse_into_the_caller_block() {
    let ir = linear(
        r"
@Def[f, [d : Dict[i64, i64], k : i64] : i64, @Block[
  @Let[x, @DictGet[d, k, 0]],
  x
]];
f[@Dict[], 0]
",
    );
    assert!(ir.contains("= prim @DictLookup"), "同步原语应融合为指令: {ir}");
}

#[test]
fn asynchronous_primitives_keep_their_continuation() {
    let ir = linear(
        r"
@Def[f, [xs : List[i64]] : List[i64],
  @ListMap[xs, @Fn[[x : i64] : i64, AddI64[x, 1]]]
];
f[{1}]
",
    );
    assert!(!ir.contains("= prim @ListMap"), "高阶原语不应融合: {ir}");
    assert!(ir.contains("prim @ListMap"), "高阶原语应保留: {ir}");
}

#[test]
fn long_list_linearizes_without_call_stack() {
    let items = (0..2000).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let ir = linear(&format!("@Let[xs, {{{items}}}]; xs"));
    assert!(ir.contains("fn $main("), "长列表应完成线性化");
}

#[test]
fn long_statement_sequence_linearizes_without_call_stack() {
    let mut source = String::new();
    for i in 0..2000 {
        writeln!(source, "@Let[x{i}, AddI64[{i}, 0]];").expect("写入字符串应成功");
    }
    source.push_str("x1999");
    let ir = linear(&source);
    assert!(ir.contains("fn $main("), "长语句序列应完成线性化");
}

#[test]
fn dead_slots_are_emitted_once_per_block() {
    // 每个槽位只在最后使用后的第一个原语清理一次, 后续原语不重复列出.
    let source = r"
@Def[f, [d : Dict[i64, i64], k : i64, v : i64] : i64, @Block[
  @Let[d2, @DictPut[d, k, v]],
  @Let[x, @DictGet[d2, k, 0]],
  x
]];
f[@Dict[], 0, 1]
";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = ksl_frontend::check::Session::new();
    session.check(&program).expect("脚本应通过类型检查");
    let module = linearize(&lower_program(&program, session.types()));
    let function = module
        .functions
        .iter()
        .find(|function| &*ksl_base::symbol::resolve(function.name) == "f")
        .expect("函数 f 应存在");
    let mut prims = 0;
    for block in &function.blocks {
        let mut seen = std::collections::HashSet::new();
        for instr in &block.instrs {
            let Instr::Prim { dead, .. } = instr else {
                continue;
            };
            prims += 1;
            for slot in dead {
                assert!(seen.insert(*slot), "槽位 {slot} 在同一块内重复清理");
            }
        }
    }
    assert!(prims >= 2, "测试应覆盖至少两个同步原语");
}

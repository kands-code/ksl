//! 名单一致性检查.
//!
//! ksl-catalog 的内置函数, 特殊形式, 操作与内置类型名是唯一来源.
//! tree-sitter 的语法与高亮查询各自维护一份名单.
//! 漏改不会影响解析器生成, 但会让编辑器与解释器脱节.
//! 本测试检查内置函数, 特殊形式, 操作名与内置类型名单一致.

use std::collections::BTreeSet;

use ksl_frontend::ast::{HighlightGroup, highlight_group};

/// tree-sitter 语法.
const GRAMMAR: &str = include_str!("../grammar.js");

/// 高亮查询.
const HIGHLIGHTS: &str = include_str!("../queries/highlights.scm");

/// MT 管理器的语法文件, 即 `.helix/ksl.mtsx`.
const MT_SYNTAX: &str = include_str!("../.helix/ksl.mtsx");

/// 高亮查询内置函数名单起始标记.
const BUILTIN_BEGIN: &str = "; BEGIN builtins";

/// 高亮查询内置函数名单结束标记.
const BUILTIN_END: &str = "; END builtins";

/// 高亮查询特殊形式名单起始标记.
const SPECIAL_FORM_BEGIN: &str = "; BEGIN special forms";

/// 高亮查询特殊形式名单结束标记.
const SPECIAL_FORM_END: &str = "; END special forms";

/// 高亮查询容器操作名单起始标记.
const CONTAINER_BEGIN: &str = "; BEGIN container operations";

/// 高亮查询容器操作名单结束标记.
const CONTAINER_END: &str = "; END container operations";

/// 操作族数组名.
const OP_ARRAYS: &[&str] = &[
    "LIST_OPS",
    "PAIR_OPS",
    "DICT_OPS",
    "SET_OPS",
    "OPTION_OPS",
    "REGISTRY_OPS",
];

/// ksl-catalog 的内置函数名.
fn builtin_names() -> BTreeSet<String> {
    ksl_catalog::builtin::names()
        .into_iter()
        .map(|symbol| ksl_base::symbol::resolve(symbol).to_string())
        .collect()
}

/// ksl-catalog 的操作名.
fn core_names() -> BTreeSet<String> {
    ksl_frontend::ast::operation_names()
        .map(str::to_string)
        .collect()
}

/// ksl-catalog 的特殊形式名.
///
/// 形式表是特殊形式名的唯一来源; ksl-catalog 新增形式而 tree-sitter 漏改时在此报出.
fn core_special_form_names() -> BTreeSet<String> {
    ksl_frontend::ast::special_form_names()
        .map(str::to_string)
        .collect()
}

/// 从高亮查询的内置函数区间提取名字.
fn query_builtin_names() -> BTreeSet<String> {
    let begin = HIGHLIGHTS.find(BUILTIN_BEGIN).expect("查询缺少 BEGIN 标记");
    let end = HIGHLIGHTS.find(BUILTIN_END).expect("查询缺少 END 标记");
    HIGHLIGHTS[begin..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// 从 grammar.js 的数组提取单引号字符串.
fn grammar_array(name: &str) -> BTreeSet<String> {
    let marker = format!("const {name} = [");
    let start = GRAMMAR
        .find(&marker)
        .unwrap_or_else(|| panic!("grammar.js 缺少 {name}"));
    let rest = &GRAMMAR[start..];
    let end = rest.find("];").unwrap_or_else(|| panic!("{name} 缺少结束标记"));
    rest[..end]
        .split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// grammar.js 中全部操作名.
fn grammar_names() -> BTreeSet<String> {
    OP_ARRAYS.iter().flat_map(|name| grammar_array(name)).collect()
}

/// 从高亮查询的标记区间提取名字.
fn query_names(begin: &str, end: &str) -> BTreeSet<String> {
    let begin = HIGHLIGHTS.find(begin).expect("高亮查询缺少 BEGIN 标记");
    let end = HIGHLIGHTS.find(end).expect("高亮查询缺少 END 标记");
    HIGHLIGHTS[begin..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// 按文档分组提取 ksl-catalog 的名字.
fn core_group(group: HighlightGroup) -> BTreeSet<String> {
    core_special_form_names()
        .into_iter()
        .chain(core_names())
        .filter(|name| highlight_group(name) == Some(group))
        .collect()
}

/// 从 MT 语法文件的标记区间提取名字.
fn mt_names(begin: &str, end: &str) -> BTreeSet<String> {
    let begin = MT_SYNTAX.find(begin).expect("MT 语法缺少 BEGIN 标记");
    let end = MT_SYNTAX.find(end).expect("MT 语法缺少 END 标记");
    MT_SYNTAX[begin..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .flat_map(|line| line.split_whitespace())
        .map(str::to_string)
        .collect()
}

/// 比较两份名单; `hint` 指向对应的生成脚本.
fn assert_same(
    label: &str,
    expected: &BTreeSet<String>,
    actual: &BTreeSet<String>,
    hint: &str,
) {
    let missing: Vec<_> = expected.difference(actual).collect();
    let extra: Vec<_> = actual.difference(expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{label}不一致, {hint}.\n应有而缺失: {missing:?}\n存在而多余: {extra:?}"
    );
}

/// 内置函数与内置类型以及操作名与特殊形式的生成名单必须与 ksl-catalog 一致.
#[test]
fn generated_lists_match_core() {
    let builtins = builtin_names();
    assert_same(
        "高亮查询内置函数名单",
        &builtins,
        &query_builtin_names(),
        "请运行 scripts/gen-builtins.sh 重新生成",
    );
    assert_same(
        "MT 内置函数名单",
        &builtins,
        &mt_names("// BEGIN builtins", "// END builtins"),
        "请运行 scripts/gen-mtsx.sh 重新生成",
    );

    let types: BTreeSet<String> = ksl_frontend::check::builtin_type_names()
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    assert_same(
        "MT 内置类型名单",
        &types,
        &mt_names("// BEGIN types", "// END types"),
        "请运行 scripts/gen-mtsx.sh 重新生成",
    );

    let operations = core_names();
    assert_same(
        "grammar.js 操作名",
        &operations,
        &grammar_names(),
        "请同步 grammar.js",
    );

    let special_forms = core_group(HighlightGroup::SpecialForm);
    let container_operations = core_group(HighlightGroup::ContainerOperation);
    assert_same(
        "highlights.scm 特殊形式名单",
        &special_forms,
        &query_names(SPECIAL_FORM_BEGIN, SPECIAL_FORM_END),
        "请同步 queries/highlights.scm",
    );
    assert_same(
        "highlights.scm 容器操作名单",
        &container_operations,
        &query_names(CONTAINER_BEGIN, CONTAINER_END),
        "请同步 queries/highlights.scm",
    );
    assert_same(
        "ksl.mtsx 特殊形式名单",
        &special_forms,
        &mt_names("// BEGIN special forms", "// END special forms"),
        "请运行 scripts/gen-mtsx.sh 重新生成",
    );
    assert_same(
        "ksl.mtsx 操作名",
        &container_operations,
        &mt_names("// BEGIN operations", "// END operations"),
        "请运行 scripts/gen-mtsx.sh 重新生成",
    );
}

/// 语法必须接纳 ksl-catalog 的全部特殊形式名.
///
/// 形式名在语法里以字面量出现, 故按字面量核对.
#[test]
fn grammar_accepts_every_special_form_name() {
    for name in core_special_form_names() {
        assert!(
            GRAMMAR.contains(&format!("'{name}'")),
            "grammar.js 缺少特殊形式名 {name}"
        );
    }
}

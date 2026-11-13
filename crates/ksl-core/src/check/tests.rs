//! check 模块测试.
//!
//! 覆盖检查器边界, 错误路径与类型表; 常规语言层功能由脚本用例覆盖.

use super::*;
use crate::ast::{ExprKind, Special, TypeExpr};
use crate::error::{ErrorKind, KslError};

fn check_str(source: &str) -> Result<(), KslError> {
    crate::testutil::check(source)
}

#[test]
fn expected_types_propagate_through_annotations_and_operations() {
    // 整数字面量严格按后缀定型: u8 必须写 u 后缀.
    assert!(check_str("@Let[x : u8, 12u];").is_ok());
    assert!(check_str("@Let[x : u8, 12];").is_err());
    assert!(check_str("@Let[x : u8, 300];").is_err());
    // 形状不符的标注仍报错: 浮点字面量不能解释为 u8.
    let err = check_str("@Let[x : u8, 1.5];").unwrap_err();
    assert!(matches!(err.kind, ErrorKind::TypeMismatch { .. }));
    // 实参按形参类型检查; u8 实参必须带后缀.
    assert!(check_str("@Let[x, AddU8[1u, 2u]];").is_ok());
    assert!(check_str("@Let[x, AddU8[1, 2]];").is_err());
    // 列表元素按元素类型检查.
    assert!(check_str("@Let[l : List[u8], {1u, 2u}];").is_ok());
    assert!(check_str("@Let[l : List[u8], {1, 2}];").is_err());
    // 函数返回标注作为期望.
    assert!(check_str("@Let[f : Fn[[] : u8], @Fn[[] : u8, 1u]];").is_ok());
    assert!(check_str("@Let[f : Fn[[] : u8], @Fn[[] : u8, 1]];").is_err());
    // 空列表需要元素类型来源; 无标注的情形由脚本用例覆盖.
    assert!(check_str("@Let[e : List[i64], {}];").is_ok());
    // Exit 的退出码同样要求 u8 后缀.
    assert!(check_str("@Exit[0];").is_err());
    assert!(check_str("@Exit[0u];").is_ok());

    // 配对分量按标注检查; 整数字面量不会因 f64 期望自动转为浮点.
    assert!(check_str("@Let[p : Pair[u8, i64], (255u, 256)];").is_ok());
    assert!(check_str("@Let[p : Pair[u8, i64], (255, 256)];").is_err());
    assert!(check_str("@Let[p : Pair[u8, f64], (1u, 2.0)];").is_ok());
    assert!(check_str("@Let[p : Pair[u8, f64], (1, 2.0)];").is_err());
    assert!(check_str("@Let[p : Pair[f64, i64], (1, 2)];").is_err());

    // 取元素, 下标必须 u64.
    assert!(check_str("@Let[x, @ListGet[{1, 2}, 0U]];").is_ok());
    assert!(check_str("@Let[x, @ListGet[{1, 2}, 0]];").is_err());
    assert!(check_str("@Let[x, @ListGet[{1, 2}, 0u]];").is_err());
    // Map 的函数签名决定结果元素类型.
    assert!(
        check_str(
            r#"@Let[f : Fn[[i64] : string], @Fn[[n : i64] : string, "x"]];
               @Let[y, @ListMap[{1, 2}, f]];"#
        )
        .is_ok()
    );
    // Filter 谓词参数必须匹配元素类型.
    assert!(
        check_str(
            r"@Let[g : Fn[[i64] : bool], @Fn[[n : i64] : bool, true]];
               @Let[y, @ListFilter[{'a'}, g]];"
        )
        .is_err()
    );
    // Fold: 初值类型经函数签名贯通.
    assert!(
        check_str(
            r"@Let[h : Fn[[i64, i64] : i64], @Fn[[a : i64, b : i64] : i64, a]];
               @Let[y, @ListFold[{1, 2}, h, 0]];"
        )
        .is_ok()
    );
    // Fst 取配对第一位, 与标注不符时报错.
    assert!(check_str("@Let[p, (1, 'a')]; @Let[f, @Fst[p]];").is_ok());
    assert!(check_str("@Let[p, (1, 'a')]; @Let[f : char, @Fst[p]];").is_err());
    // 参数个数.
    assert!(check_str("@Let[x, @ListHead[{1, 2}, 0]];").is_err());

    // 标注给出的期望类型沿列表操作反向传播.
    let fold = "@Fn[[a : u8, b : u8] : u8, a]";
    assert!(check_str(&format!("@Let[x : u8, @ListFold[{{1u}}, {fold}, 1u]];")).is_ok());
    assert!(check_str(&format!("@Let[x : u8, @ListFold[{{1u}}, {fold}, 1]];")).is_err());
    assert!(check_str("@Let[x : List[u8], @ListTail[{1u, 2u}]];").is_ok());
    assert!(check_str("@Let[x : List[u8], @ListTail[{1, 2}]];").is_err());
    assert!(
        check_str("@Let[x : List[u8], @ListMap[{1u, 2u}, @Fn[[n : u8] : u8, n]]];").is_ok()
    );
    assert!(check_str("@Let[x : List[u8], @ListMap[{1, 2}, @Fn[[n : u8] : u8, n]]];").is_err());
    assert!(check_str("@Let[x : u8, @ListHead[{1u, 2u}]];").is_ok());
    assert!(check_str("@Let[x : u8, @ListHead[{1, 2}]];").is_err());
}

#[test]
fn inference_unifies_branches_lists_and_generic_constructors() {
    // `If :: bool -> a -> a -> a`: 两支共享类型变量, 先定型者给另一支作期望.
    assert!(
        check_str("@Let[c, true]; @Let[a, @If[c, Option/Some[1], Option/None[]]];").is_ok()
    );
    // 先查的一支自身无法定型时, 改由另一支定型后回头检查.
    assert!(
        check_str("@Let[c, true]; @Let[a, @If[c, Option/None[], Option/Some[1]]];").is_ok()
    );
    // 两支都无依据时仍报错.
    let err =
        check_str("@Let[c, true]; @Let[a, @If[c, Option/None[], Option/None[]]];").unwrap_err();
    assert!(matches!(err.kind, ErrorKind::UndeterminedType { .. }));
    // `@Match` 各分支体共享结果类型, 取第一条分支.
    assert!(
        check_str(
            "@Let[a, Option/Some[1]]; @Let[m, @Match[a, {(/Some[v], v), (/None[], 0)}]];"
        )
        .is_ok()
    );
    // never 分支不阻止后续分支定型.
    let handler_if = r"
        @Let[h, @Handler[[self : Pid[i64, i64]] : i64,
          @Block[
            @Let[x, @If[false, @Halt[], 1]],
            x
          ]]];
    ";
    assert!(check_str(handler_if).is_ok());
    let handler_match = r"
        @Let[h, @Handler[[self : Pid[i64, i64]] : i64,
          @Block[
            @Let[x, @Match[Option/Some[1], {
              (Option/Some[n], @Halt[]),
              (Option/None[], 1)
            }]],
            x
          ]]];
    ";
    assert!(check_str(handler_match).is_ok());
    assert!(
        check_str(
            r"@Let[h, @Handler[[self : Pid[i64, i64]] : i64,
                @If[false, @Halt[], @Halt[]]]];"
        )
        .is_ok()
    );

    // 分支结果与列表元素共享类型变量; 空列表可由另一支或相邻元素定型.
    assert!(check_str("@Let[c, true]; @Let[x, @If[c, {}, {1}]];").is_ok());
    assert!(check_str("@Let[c, true]; @Let[x, @If[c, {}, {}]];").is_err());
    assert!(
        check_str("@Let[x, @Match[Option/Some[1], {(/Some[n], {}), (/None[], {1})}]];").is_ok()
    );
    assert!(
        check_str("@Let[x, @Match[Option/Some[1], {(/Some[n], {}), (/None[], {})}]];").is_err()
    );
    assert!(check_str("@Let[x, {Option/None[], Option/Some[1]}];").is_ok());
    assert!(check_str("@Let[x, {Option/Some[1], Option/None[]}];").is_ok());
    assert!(check_str("@Let[x, {1, 2u}];").is_err());
    assert!(check_str("@Let[x, {1u, 2}];").is_err());
    assert!(check_str("@Let[x, {Option/None[], Option/None[]}];").is_err());
    assert!(check_str("@Let[x, {1, 2}];").is_ok());
    assert!(check_str("@Let[x, {1u, 2u}];").is_ok());

    // 期望给出未定参数时构造成立.
    assert!(check_str("@Let[op : Option[i64], Option/None[]];").is_ok());
    assert!(check_str("@Let[r : Result[i64, string], Result/Ok[10]];").is_ok());
    assert!(check_str(r#"@Let[r : Result[i64, string], Result/Err["e"]];"#).is_ok());
    // 无期望时未定参数无从确定, 报错并要求标注.
    let err = check_str("@Let[op, Option/None[]];").unwrap_err();
    assert!(matches!(err.kind, ErrorKind::UndeterminedType { .. }));
    let err = check_str("@Let[r, Result/Ok[10]];").unwrap_err();
    assert!(matches!(err.kind, ErrorKind::UndeterminedType { .. }));
    // 用户 Data 的构造器调用.
    assert!(check_str(r#"@Data[P, {Init[string, u8]}]; @Let[p, P/Init["k", 10u]];"#).is_ok());
}

#[test]
fn functions_definitions_and_fix() {
    // 内置调用的实参类型与个数.
    assert!(check_str("@Let[x, AddI64[1, 2]];").is_ok());
    assert!(check_str("@Let[x, AddI64[1, 2.0]];").is_err());
    assert!(check_str("@Let[x, AddI64[1]];").is_err());
    // 用户函数的定义与调用.
    let source = r"
        @Let[addOne : Fn[[i64] : i64], @Fn[[x : i64] : i64, AddI64[x, 1]]];
        @Let[y, addOne[2]];
    ";
    assert!(check_str(source).is_ok());
    // 函数体类型与返回标注不符.
    assert!(check_str(r#"@Let[f, @Fn[[x : i64] : i64, "s"]];"#).is_err());
    // 实参个数不符.
    let source = r"
        @Let[addOne : Fn[[i64] : i64], @Fn[[x : i64] : i64, AddI64[x, 1]]];
        @Let[y, addOne[2, 3]];
    ";
    assert!(check_str(source).is_err());
    // 未定义名字报错.
    assert!(check_str("@Let[x, nosuch];").is_err());

    // 具名函数默认自递归.
    let source = r"
        @Def[fact, [n : i64] : i64,
          @If[LessOrEqualI64[n, 1], 1, MulI64[n, fact[SubI64[n, 1]]]]];
        @Let[x, fact[5]];
    ";
    assert!(check_str(source).is_ok());
    // 组内相互递归.
    let source = r"
        @Fix[
          @Def[isEven, [n : u8] : bool, @If[EqualU8[n, 0u], true, isOdd[SubU8[n, 1u]]]],
          @Def[isOdd, [n : u8] : bool, @If[EqualU8[n, 0u], false, isEven[SubU8[n, 1u]]]]
        ];
    ";
    assert!(check_str(source).is_ok());
    // 参数名重复报错.
    assert!(matches!(
        check_str("@Def[f, [x : i64, x : i64] : i64, x];")
            .unwrap_err()
            .kind,
        ErrorKind::DuplicateParam
    ));
    // 组内重名报错.
    let duplicate = r"
        @Fix[
          @Def[f, [n : i64] : i64, n],
          @Def[f, [n : i64] : i64, n]
        ];
    ";
    assert!(matches!(
        check_str(duplicate).unwrap_err().kind,
        ErrorKind::DuplicateFixName
    ));
    // @Fix 之后的同块语句可用其绑定; 所在块结束后消失.
    let source = r"
        @Block[
          @Fix[@Def[helper, [n : i64] : i64, AddI64[n, 1]]],
          @Let[y, helper[1]]
        ];
    ";
    assert!(check_str(source).is_ok());
    let source = r"
        @Block[@Fix[@Def[helper, [n : i64] : i64, n]]];
        @Let[y, helper[1]];
    ";
    assert!(check_str(source).is_err());
}

#[test]
fn actor_types_and_handlers() {
    // 最后一个类型参数是返回类型, 前面是信箱消息类型.
    assert!(check_str("@Def[f, [p : Pid[i64, string]] : nil, nil];").is_ok());
    assert!(check_str("@Def[f, [p : Pid[i64, f64, string]] : nil, nil];").is_ok());
    // 至少一个信箱.
    assert!(matches!(
        check_str("@Def[f, [p : Pid[string]] : nil, nil];")
            .unwrap_err()
            .kind,
        ErrorKind::InvalidPidType { mailboxes: 0 }
    ));
    // 最多 16 个信箱.
    let many = std::iter::repeat_n("i64", 17).collect::<Vec<_>>().join(", ");
    let source = format!("@Def[f, [p : Pid[{many}, string]] : nil, nil];");
    assert!(matches!(
        check_str(&source).unwrap_err().kind,
        ErrorKind::InvalidPidType { mailboxes: 17 }
    ));

    // Handler 首参数必须是 Pid; 其余参数是状态类型.
    assert!(check_str("@Def[f, [h : Handler[Pid[i64, string]]] : nil, nil];").is_ok());
    assert!(check_str("@Def[f, [h : Handler[Pid[i64, string], i64]] : nil, nil];").is_ok());
    assert!(matches!(
        check_str("@Def[f, [h : Handler[i64]] : nil, nil];")
            .unwrap_err()
            .kind,
        ErrorKind::InvalidHandlerType
    ));
    // ExitReason 是内置 Data, 可构造与穷尽匹配.
    let source = r"
        @Def[code, [e : ExitReason] : i64,
          @Match[e, {
            (/Halted[], 0),
            (/Killed[], 1),
            (/Failed[c, m], c)
          }]];
    ";
    assert!(check_str(source).is_ok());
    assert!(check_str("@Let[e, ExitReason/Halted[]];").is_ok());
    // Handler 表达式与 Continue.
    assert!(
        check_str(r#"@Let[h, @Handler[[self : Pid[i64, string], n : i64] : string, "ok"]];"#)
            .is_ok()
    );
    assert!(
        check_str(
            r"@Let[h, @Handler[[self : Pid[i64, string], n : i64] : string,
                @Continue[1]]];"
        )
        .is_ok()
    );
    assert!(matches!(
        check_str("@Let[x, @Continue[]];").unwrap_err().kind,
        ErrorKind::ActorPrimitiveOutsideHandler
    ));
    assert!(
        check_str(
            r#"@Let[h, @Handler[[self : Pid[i64, string], n : i64] : string,
                @Continue["x"]]];"#
        )
        .is_err()
    );
    assert!(matches!(
        check_str("@Let[h, @Handler[[n : i64] : i64, 0]];")
            .unwrap_err()
            .kind,
        ErrorKind::InvalidHandlerParam
    ));
    assert!(matches!(
        check_str("@Let[h, @Handler[[self : Pid[i64, string]] : i64, 0]];")
            .unwrap_err()
            .kind,
        ErrorKind::TypeMismatch { .. }
    ));
}

#[test]
fn actor_primitives_type_check() {
    let source = r#"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[h, @Handler[[self : Pid[i64, string], n : i64] : string, "ok"]],
            @Let[p, @Spawn[h, 0]],
            @Let[x, @Send[p, 0u, 1]],
            @Let[r, @Join[p]],
            @Let[k, @Kill[p]]
          ]]
    "#;
    assert!(check_str(source).is_ok());
    // handler 必须是 Handler 类型; 信箱数由 Handler 的 Pid 类型推断.
    assert!(matches!(
        check_str(
            r"@Main[[self : Pid[nil, nil]] : nil,
                @Block[@Let[p, @Spawn[5]], nil]]"
        )
        .unwrap_err()
        .kind,
        ErrorKind::NotHandler { .. }
    ));
    // 索引必须是字面量且在范围内.
    assert!(matches!(
        check_str(
            r#"@Main[[self : Pid[nil, nil]] : nil,
                @Block[
                    @Let[h, @Handler[[self : Pid[i64, string]] : string, "ok"]],
                    @Let[p, @Spawn[h]],
                    @Let[x, @Send[p, "i", 1]]
                ]] "#
        )
        .unwrap_err()
        .kind,
        ErrorKind::LiteralRequired
    ));
    assert!(matches!(
        check_str(
            r#"@Main[[self : Pid[nil, nil]] : nil,
                @Block[
                    @Let[h, @Handler[[self : Pid[i64, string]] : string, "ok"]],
                    @Let[p, @Spawn[h]],
                    @Let[x, @Send[p, 1u, 1]]
                ]] "#
        )
        .unwrap_err()
        .kind,
        ErrorKind::MailboxIndexOutOfRange { .. }
    ));
    // Receive/Halt/Sleep 仅限 Handler 体内.
    assert!(matches!(
        check_str("@Let[x, @Receive[0u, Option/None[]]];")
            .unwrap_err()
            .kind,
        ErrorKind::ActorPrimitiveOutsideHandler
    ));
    assert!(matches!(
        check_str("@Let[x, @Halt[]];").unwrap_err().kind,
        ErrorKind::ActorPrimitiveOutsideHandler
    ));
    assert!(matches!(
        check_str("@Let[x, @Sleep[1U]];").unwrap_err().kind,
        ErrorKind::ActorPrimitiveOutsideHandler
    ));
    // Handler 内可用 Receive/Sleep/Continue.
    let handler = r#"
        @Let[h, @Handler[[self : Pid[i64, string], n : i64] : string,
          @Block[
            @Let[msg, @Receive[0u, Option/None[]]],
            @Sleep[1U],
            @Match[msg, {
              (/Some[m], @Continue[AddI64[n, m]]),
              (/None[], "done")
            }]
          ]]];
    "#;
    assert!(check_str(handler).is_ok());

    // 信箱数与索引是 u8 字面量; 无 u 后缀的整数字面量是 i64.
    let mailboxes = r"
        @Main[[self : Pid[nil, nil]] : nil,
          @Block[
            @Let[h, @Handler[[self : Pid[i64, i64]] : i64,
              @Match[@Receive[0u, Option/None[]], {
                (/Some[x], x),
                (/None[], 0)
              }]]],
            @Let[p, @Spawn[h]]
          ]]
    ";
    assert!(check_str(mailboxes).is_ok());
    let no_suffix = mailboxes.replace("0u", "0").replace("1u", "1");
    assert!(matches!(
        check_str(&no_suffix).unwrap_err().kind,
        ErrorKind::TypeMismatch { .. }
    ));
}

#[test]
fn actor_primitive_context_restrictions() {
    // @Main 只能有一个 self 参数并返回 nil.
    assert!(matches!(
        check_str("@Main[[self : Pid[nil, nil], extra : i64] : nil, nil]")
            .unwrap_err()
            .kind,
        ErrorKind::InvalidMainParam
    ));
    assert!(matches!(
        check_str("@Main[[self : Pid[nil, nil]] : i64, 1]")
            .unwrap_err()
            .kind,
        ErrorKind::InvalidMainReturn
    ));
    // @Halt 与 @Continue 不能在 Main 体内使用; @Exit 可以.
    assert!(matches!(
        check_str("@Main[[self : Pid[nil, nil]] : nil, @Halt[]]")
            .unwrap_err()
            .kind,
        ErrorKind::HaltInMain
    ));
    assert!(matches!(
        check_str("@Main[[self : Pid[nil, nil]] : nil, @Continue[]]")
            .unwrap_err()
            .kind,
        ErrorKind::ContinueInMain
    ));
    assert!(check_str("@Main[[self : Pid[nil, nil]] : nil, @Exit[0u]]").is_ok());
    // Actor 原语在 Handler/Main 之外一律禁止.
    for source in [
        "@Let[h, @Handler[[self : Pid[i64, i64]] : i64, 0]]; @Spawn[h];",
        "@Let[h, @Handler[[self : Pid[i64, i64]] : i64, 0]]; @Let[x, @Join[@Spawn[h]]];",
        "@Register[\"x\", @Spawn[@Handler[[self : Pid[i64, i64]] : i64, 0]]];",
    ] {
        assert!(matches!(
            check_str(source).unwrap_err().kind,
            ErrorKind::ActorPrimitiveOutsideHandler
        ));
    }
}

#[test]
fn data_declarations_and_reserved_names() {
    // 重复类型, 重复变体, 保留名均报错.
    assert!(matches!(
        check_str("@Data[D, {A[]}]; @Data[D, {B[]}];").unwrap_err().kind,
        ErrorKind::DuplicateData
    ));
    assert!(matches!(
        check_str("@Data[D, {A[], A[i64]}];").unwrap_err().kind,
        ErrorKind::DuplicateVariant
    ));
    assert!(matches!(
        check_str("@Data[List, {A[]}];").unwrap_err().kind,
        ErrorKind::ReservedName
    ));
    // 内置 Data 变体名不保留, 用户变体通过类型限定名区分.
    assert!(check_str("@Data[D, {Some[]}];").is_ok());
    // 内置函数名保留.
    assert!(matches!(
        check_str("@Data[D, {AddI64[]}];").unwrap_err().kind,
        ErrorKind::ReservedName
    ));
    // 正常声明可用.
    assert!(check_str("@Data[D, {A[], B[i64]}];").is_ok());
    // 空 Data 声明报错.
    assert!(matches!(
        check_str("@Data[D, {}];").unwrap_err().kind,
        ErrorKind::EmptyData
    ));

    // 保留名不能作为绑定名.
    assert!(check_str("@Let[AddI64, 5];").is_err());
    assert!(check_str("@Let[List, 5];").is_err());
    assert!(check_str("@Let[true, 5];").is_err());
    assert!(check_str("@Def[ListMap, [x : i64] : i64, x];").is_err());
    assert!(check_str("@Let[x, 5]; @Let[y, 6];").is_ok());
}

#[test]
fn match_exhaustiveness_and_pattern_fields() {
    let ok = r"
        @Data[S, {P[], C[f64]}];
        @Let[p, S/C[3.2]];
        @Let[a, @Match[p, {(/P[], 0.0), (/C[r], r)}]];
    ";
    assert!(check_str(ok).is_ok());
    // 漏写变体.
    let inexhaustive = r"
        @Data[S, {P[], C[f64]}];
        @Let[p, S/C[3.2]];
        @Let[a, @Match[p, {(/C[r], r)}]];
    ";
    assert!(matches!(
        check_str(inexhaustive).unwrap_err().kind,
        ErrorKind::NonExhaustiveMatch { .. }
    ));
    // 分支类型不一致.
    let mismatch = r"
        @Data[S, {P[], C[f64]}];
        @Let[p, S/C[3.2]];
        @Let[a, @Match[p, {(/P[], 0.0), (/C[r], 1)}]];
    ";
    assert!(check_str(mismatch).is_err());
    // Option 名义上属于 Data, 可匹配.
    let opt = r#"@Let[a, @Match[GetEnv["PATH"], {(/Some[s], s), (/None[], "unset")}]];"#;
    assert!(check_str(opt).is_ok());
    // 通配分支匹配任意变体, 且满足穷尽性.
    let wildcard = r"
        @Data[S, {P[], C[f64]}];
        @Let[p, S/C[3.2]];
        @Let[a, @Match[p, {(/C[x], Option/Some[x]), (_, Option/None[])}]];
    ";
    assert!(check_str(wildcard).is_ok());
    // 通配分支之后不可再有分支.
    let unreachable = r"
        @Data[S, {P[], C[f64]}];
        @Let[p, S/C[3.2]];
        @Let[a, @Match[p, {(_, 0), (/C[x], 1)}]];
    ";
    assert!(check_str(unreachable).is_err());

    // 模式字段是绑定: 保留名不能作字段, 同一分支内字段名不能重复.
    let source = r"
        @Data[D, {C[i64, i64]}];
        @Let[v, D/C[1, 2]];
    ";
    assert!(matches!(
        check_str(&format!(
            "{source} @Let[z, @Match[v, {{(/C[AddI64, _], AddI64)}}]];"
        ))
        .unwrap_err()
        .kind,
        ErrorKind::ReservedName
    ));
    assert!(matches!(
        check_str(&format!("{source} @Let[z, @Match[v, {{(/C[x, x], x)}}]];"))
            .unwrap_err()
            .kind,
        ErrorKind::DuplicatePatternField { .. }
    ));
}

#[test]
fn format_spec_is_checked() {
    assert!(check_str(r#"@Let[a, f"{1:.2} {2u:x} {3.0:e} {true}"];"#).is_ok());
    assert!(check_str(r#"@Let[s, "hi"]; @Let[c, 'z']; @Let[a, f"{s:?} {c:?}"];"#).is_ok());
    // `?` 仅限 string 与 char.
    assert!(check_str(r#"@Let[a, f"{1:?}"];"#).is_err());
    // 数值格式仅限数值类型.
    assert!(check_str(r#"@Let[s, "hi"]; @Let[a, f"{s:.2}"];"#).is_err());
    // 非法格式字符.
    assert!(check_str(r#"@Let[a, f"{1:w}"];"#).is_err());
    // 类型不允许的格式, 空格式参数与八进制均报错.
    assert!(check_str(r#"@Let[a, f"{s:}"];"#).is_err());
    assert!(check_str(r#"@Let[a, f"{1:e}"];"#).is_err());
    assert!(check_str(r#"@Let[a, f"{1.5:x}"];"#).is_err());
    assert!(check_str(r#"@Let[a, f"{1.0:#}"];"#).is_err());
    assert!(check_str(r#"@Let[a, f"{8:#o}"];"#).is_err());
    // 不可打印类型不能进入 f-string.
    assert!(check_str(r#"@Data[D, {C[]}]; @Let[v, D/C[]]; @Let[a, f"{v}"];"#).is_err());
    assert!(check_str(r#"@Let[o : Option[i64], Option/Some[1]]; @Let[a, f"{o}"];"#).is_err());
    assert!(check_str(r#"@Let[r, OpenRead["x"]]; @Let[s, f"{r}"];"#).is_err());
    // 宽度与精度上限.
    assert!(check_str(r#"@Let[x, f"{1:256}"];"#).is_ok());
    assert!(check_str(r#"@Let[x, f"{1:257}"];"#).is_err());
    assert!(check_str(r#"@Let[x, f"{1.0:.24}"];"#).is_ok());
    assert!(check_str(r#"@Let[x, f"{1.0:.25}"];"#).is_err());
}

#[test]
fn option_and_result_helper_types() {
    // 判定返回 bool; 映射按函数签名产出容器.
    assert!(check_str("@Let[o : Option[i64], Option/Some[1]]; @Let[b, @IsSome[o]];").is_ok());
    assert!(
        check_str("@Let[r : Result[i64, string], Result/Ok[1]]; @Let[b, @IsOk[r]];").is_ok()
    );
    assert!(
        check_str(
            "@Let[o : Option[i64], Option/Some[1]];
             @Let[m, @OptionMap[o, @Fn[[x : i64] : string, \"s\"]]];"
        )
        .is_ok()
    );
    // 元素类型不符即报错.
    assert!(
        check_str(
            "@Let[o : Option[i64], Option/Some[1]]; @Let[m, @OptionMap[o, @Fn[[x : string] : \
             i64, x]]];"
        )
        .is_err()
    );
    // 默认值类型须与元素类型一致.
    assert!(
        check_str("@Let[o : Option[i64], Option/None[]]; @Let[x, @OptionGet[o, 42]];").is_ok()
    );
    assert!(
        check_str("@Let[o : Option[i64], Option/None[]]; @Let[x, @OptionGet[o, \"s\"]];")
            .is_err()
    );
    // 绑定的函数须返回同类容器.
    assert!(
        check_str(
            "@Let[o : Option[i64], Option/Some[1]]; @Let[m, @OptionAndThen[o, @Fn[[x : i64] : \
             i64, x]]];"
        )
        .is_err()
    );
    // 少参与多参都应报错.
    assert!(matches!(
        check_str("@Let[x, @IsSome[]];").unwrap_err().kind,
        ErrorKind::ArityMismatch { .. }
    ));
    assert!(matches!(
        check_str("@Let[o : Option[i64], Option/Some[1]]; @Let[x, @IsSome[o, o]];")
            .unwrap_err()
            .kind,
        ErrorKind::ArityMismatch { .. }
    ));
    assert!(matches!(
        check_str("@Let[o : Option[i64], Option/Some[1]]; @Let[x, @OptionMap[o]];")
            .unwrap_err()
            .kind,
        ErrorKind::ArityMismatch { .. }
    ));
}

#[test]
fn values_must_be_used_and_determined() {
    assert!(check_str("@Let[_, 5];").is_ok());
    assert!(check_str("5;").is_err());
    // `@Block` 内的非末句同样不能隐式丢弃.
    assert!(check_str("@Block[5, nil];").is_err());
    assert!(check_str("@Block[@Let[_, 5], nil];").is_ok());
    // 被丢弃的值仍须能定型.
    assert!(check_str("@Let[_, {}];").is_err());
    assert!(check_str("@Let[_, Option/None[]];").is_err());
    assert!(check_str("@Let[_, {Option/None[], Option/None[]}];").is_err());
    assert!(check_str("@Let[_, {1, 2u}];").is_err());
    assert!(check_str("@Let[_, {1, 2}];").is_ok());
}

#[test]
fn type_tables_record_expressions_data_and_types() {
    let source = r"
        @Let[x : i64, 1];
        @Let[f, @Fn[[n : i64] : i64, AddI64[n, 1]]];
        @Let[y, f[x]];
    ";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过检查");
    let types = session.types();

    let ExprKind::Special(Special::Let { target, value }) = &program.stmts[0].node else {
        panic!("首条语句应为 @Let");
    };
    let name = target.name.as_ref().expect("绑定名应存在");
    assert_eq!(types.get(&name.key()), Some(&Type::I64));
    assert_eq!(types.get(&value.key()), Some(&Type::I64));

    let ExprKind::Special(Special::Let { target, value }) = &program.stmts[1].node else {
        panic!("第二条语句应为 @Let");
    };
    let name = target.name.as_ref().expect("绑定名应存在");
    let Type::Fn(params, ret) = types.get(&name.key()).expect("函数名应有类型") else {
        panic!("函数名应为函数类型");
    };
    assert_eq!(params.as_slice(), &[Type::I64]);
    assert_eq!(**ret, Type::I64);
    assert_eq!(types.get(&value.key()), types.get(&name.key()));

    let ExprKind::Special(Special::Let { target, value }) = &program.stmts[2].node else {
        panic!("第三条语句应为 @Let");
    };
    let name = target.name.as_ref().expect("绑定名应存在");
    assert_eq!(types.get(&name.key()), Some(&Type::I64));
    assert_eq!(types.get(&value.key()), Some(&Type::I64));

    // 推断变量最终化后写入类型表.
    let source = "@Let[x, {Option/None[], Option/Some[1]}];";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过检查");
    let ExprKind::Special(Special::Let { target, .. }) = &program.stmts[0].node else {
        panic!("应为 @Let");
    };
    let name = target.name.as_ref().expect("绑定名应存在");
    assert_eq!(
        session.types().get(&name.key()),
        Some(&Type::list(Type::option(Type::I64)))
    );

    // Data 声明, 变体签名与类型表达式都进入类型表.
    let source = "@Data[D, {C[Pair[i64, u8]]}]; @Let[x : List[u8], {1u}];";
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过检查");
    let types = session.types();

    let ExprKind::Special(Special::Data { name, variants }) = &program.stmts[0].node else {
        panic!("首条语句应为 @Data");
    };
    let data_ty = Type::data(name.node);
    assert_eq!(types.get(&name.key()), Some(&data_ty));
    let variant = &variants[0];
    let field_pair = Type::pair(Type::I64, Type::U8);
    let Type::Fn(fields, ret) = types.get(&variant.name.key()).expect("变体应有签名")
    else {
        panic!("变体应记录为函数签名");
    };
    assert_eq!(fields.as_slice(), &[field_pair]);
    assert_eq!(**ret, data_ty);

    let TypeExpr::Generic { name, args } = &variant.fields[0] else {
        panic!("字段应为泛型类型");
    };
    assert_eq!(types.get(&name.key()), Some(&Type::pair(Type::I64, Type::U8)));
    let TypeExpr::Named(inner) = &args[0] else {
        panic!("字段参数应为具名类型");
    };
    assert_eq!(types.get(&inner.key()), Some(&Type::I64));
    let TypeExpr::Named(inner) = &args[1] else {
        panic!("字段参数应为具名类型");
    };
    assert_eq!(types.get(&inner.key()), Some(&Type::U8));

    let ExprKind::Special(Special::Let { target, .. }) = &program.stmts[1].node else {
        panic!("第二条语句应为 @Let");
    };
    let TypeExpr::Generic { name, args } = target.ty.as_ref().expect("应有类型标注")
    else {
        panic!("标注应为泛型类型");
    };
    assert_eq!(types.get(&name.key()), Some(&Type::list(Type::U8)));
    let TypeExpr::Named(inner) = &args[0] else {
        panic!("标注参数应为具名类型");
    };
    assert_eq!(types.get(&inner.key()), Some(&Type::U8));
}

#[test]
fn type_display_covers_applications() {
    let cases = [
        (Type::list(Type::I64), "List[i64]"),
        (Type::dict(Type::Str, Type::U64), "Dict[string, u64]"),
        (Type::set(Type::Bool), "Set[bool]"),
        (Type::pair(Type::I64, Type::U8), "Pair[i64, u8]"),
        (Type::option(Type::I64), "Option[i64]"),
        (Type::result(Type::I64, Type::Str), "Result[i64, string]"),
        (
            Type::pid(vec![Type::I64, Type::Str], Type::Nil),
            "Pid[i64, string, nil]",
        ),
        (
            Type::handler(
                Type::pid(vec![Type::I64], Type::Nil),
                vec![Type::I64, Type::Str],
            ),
            "Handler[Pid[i64, nil], i64, string]",
        ),
        (Type::data(crate::symbol::intern("Shape")), "Shape"),
        (Type::file(), "File"),
        (Type::date_time(), "DateTime"),
        (Type::socket(), "Socket"),
        (Type::tcp_listener(), "TcpListener"),
    ];
    for (ty, expected) in cases {
        assert_eq!(ty.to_string(), expected);
    }
}

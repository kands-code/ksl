//! 类型检查错误的种类断言.
//!
//! 错误编号由枚举顺序生成, 不是稳定契约; 本文件按 `ErrorKind` 断言错误语义.

use ksl_frontend::check::Session;
use ksl_frontend::error::{ErrorKind, KslError};
use ksl_frontend::testutil::parse;

fn error(source: &str) -> KslError {
    let program = match parse(source) {
        Ok(program) => program,
        Err(err) => return err,
    };
    let mut session = Session::new();
    session.check(&program).expect_err("脚本应报错")
}

macro_rules! assert_error {
    ($source:expr, $pattern:pat) => {{
        let error = error(&$source);
        assert!(
            matches!(error.kind, $pattern),
            "错误种类不符: {}\n源码: {}",
            error,
            $source
        );
    }};
}

#[test]
fn annotations_fix_literal_and_element_types() {
    assert_error!("@Let[x : u8, 12];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[x : u8, 300];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[x : u8, 1.5];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[x, AddU8[1, 2]];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[l : List[u8], {1, 2}];", ErrorKind::TypeMismatch { .. });
    assert_error!(
        "@Let[f : Fn[[] : u8], @Fn[[] : u8, 1]];",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!("@Exit[0];", ErrorKind::TypeMismatch { .. });
    assert_error!(
        "@Let[p : Pair[u8, i64], (255, 256)];",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        "@Let[p : Pair[u8, f64], (1, 2.0)];",
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn operations_check_against_annotations() {
    assert_error!("@Let[x, @ListGet[{1, 2}, 0]];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[x, @ListGet[{1, 2}, 0u]];", ErrorKind::TypeMismatch { .. });
    assert_error!(
        r"
@Let[g : Fn[[i64] : bool], @Fn[[n : i64] : bool, true]];
@Let[y, @ListFilter[{'a'}, g]];
",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        "@Let[p, (1, 'a')]; @Let[f : char, @Fst[p]];",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        r"
@Let[h : Fn[[i64, i64] : i64], @Fn[[a : i64, b : i64] : i64, a]];
@Let[y, @ListFold[{1, 2}, h, 1u]];
",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        "@Let[x : List[u8], @ListTail[{1, 2}]];",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        "@Let[x : List[u8], @ListMap[{1, 2}, @Fn[[n : u8] : u8, n]]];",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        "@Let[x : u8, @ListHead[{1, 2}]];",
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn branches_and_elements_share_types() {
    assert_error!(
        "@Let[c, true]; @Let[x, @If[c, {}, {}]];",
        ErrorKind::UndeterminedType { .. }
    );
    assert_error!(
        "@Let[b, true]; @Let[_, @If[b, {}, (1, 2)]];",
        ErrorKind::InferMismatch { .. }
    );
    assert_error!(
        "@Let[x, {Option/None[], Option/None[]}];",
        ErrorKind::UndeterminedType { .. }
    );
    assert_error!("@Let[x, {1, 2u}];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[x, {1u, 2}];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[op, Option/None[]];", ErrorKind::UndeterminedType { .. });
    assert_error!("@Let[r, Result/Ok[10]];", ErrorKind::UndeterminedType { .. });
}

#[test]
fn match_checks_exhaustiveness_and_field_names() {
    assert_error!(
        r"
@Data[Either2, {Left[i64], Right[i64]}];
@Def[unwrap, [e : Either2] : i64, @Match[e, { (/Left[n], n) }]];
",
        ErrorKind::NonExhaustiveMatch { .. }
    );
    assert_error!(
        r"
@Data[Either2, {Left[i64], Right[i64]}];
@Def[unwrap, [e : Either2] : i64,
  @Match[e, { (/Left[n], n), (/Left[m], m), (/Right[k], k) }]];
",
        ErrorKind::DuplicateMatchArm { .. }
    );
    assert_error!(
        r"
@Data[Either2, {Left[i64], Right[i64]}];
@Def[unwrap, [e : Either2] : i64,
  @Match[e, { (_, 0), (/Left[n], n) }]];
",
        ErrorKind::UnreachableMatchArm
    );
    assert_error!(
        r"
@Data[D, {C[i64, i64]}];
@Let[v, D/C[1, 2]];
@Let[z, @Match[v, { (/C[x, x], x) }]];
",
        ErrorKind::DuplicatePatternField { .. }
    );
    assert_error!(
        r"
@Data[D, {C[i64, i64]}];
@Let[v, D/C[1, 2]];
@Let[z, @Match[v, { (/C[AddI64, _], AddI64) }]];
",
        ErrorKind::ReservedName
    );
    assert_error!(
        "@Let[x, 1]; @Let[y, @Match[x, { (_, 0) }]];",
        ErrorKind::NotMatchable { .. }
    );
}

#[test]
fn which_conditions_must_not_be_literals() {
    assert_error!(
        "@Which[{(false, 1)}, 0];",
        ErrorKind::WhichLiteralCondition { .. }
    );
    assert_error!(
        "@Which[{(true, 1)}, 0];",
        ErrorKind::WhichLiteralCondition { .. }
    );
    assert_error!(
        r#"@Which[{(EqualI64[1, 0], 1), (EqualI64[1, 1], "s")}, 0];"#,
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn data_declarations_reject_duplicates_and_reserved_names() {
    assert_error!("@Data[D, {A[]}]; @Data[D, {B[]}];", ErrorKind::DuplicateData);
    assert_error!("@Data[D, {A[], A[i64]}];", ErrorKind::DuplicateVariant);
    assert_error!("@Data[Empty, {}];", ErrorKind::EmptyData);
    assert_error!("@Data[D, {D[]}];", ErrorKind::VariantNameConflict);
    assert_error!("@Data[List, {A[]}];", ErrorKind::ReservedName);
    assert_error!("@Data[KeyEvent, {A[]}];", ErrorKind::ReservedName);
    assert_error!("@Data[D, {AddI64[]}];", ErrorKind::ReservedName);
}

#[test]
fn names_reject_reserved_and_duplicate_bindings() {
    assert_error!("@Let[x, nosuch];", ErrorKind::UnknownName);
    assert_error!("@Let[x : Nope, 1];", ErrorKind::UnknownType);
    assert_error!("@Let[AddI64, 5];", ErrorKind::ReservedName);
    assert_error!("@Let[List, 5];", ErrorKind::ReservedName);
    assert_error!("@Let[true, 5];", ErrorKind::ReservedName);
    assert_error!("@Def[ListMap, [x : i64] : i64, x];", ErrorKind::ReservedName);
    assert_error!(
        "@Def[f, [x : i64, x : i64] : i64, x];",
        ErrorKind::DuplicateParam
    );
    assert_error!(
        r"
@Fix[
  @Def[f, [n : i64] : i64, n],
  @Def[f, [n : i64] : i64, n]
];
",
        ErrorKind::DuplicateFixName
    );
    assert_error!(
        "@Let[value, 1]; @Let[_, value[2]];",
        ErrorKind::NotCallable { .. }
    );
    assert_error!(
        "@Let[x, @ListHead[{1, 2}, 0]];",
        ErrorKind::ArityMismatch { .. }
    );
    assert_error!("@Let[x : List[i64, i64], 0];", ErrorKind::ArityMismatch { .. });
}

#[test]
fn functions_and_fix_have_scoped_definitions() {
    assert_error!(
        "@Let[f, @Fn[[x : i64] : i64, \"s\"]];",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        r"
@Let[addOne : Fn[[i64] : i64], @Fn[[x : i64] : i64, AddI64[x, 1]]];
@Let[y, addOne[2, 3]];
",
        ErrorKind::ArityMismatch { .. }
    );
    assert_error!(
        r"
@Block[@Fix[@Def[helper, [n : i64] : i64, n]]];
@Let[y, helper[1]];
",
        ErrorKind::UnknownName
    );
}

#[test]
fn pid_and_handler_types_are_validated() {
    assert_error!(
        "@Def[f, [p : Pid[string]] : nil, nil];",
        ErrorKind::InvalidPidType { .. }
    );
    assert_error!(
        "@Def[f, [h : Handler[i64]] : nil, nil];",
        ErrorKind::InvalidHandlerType
    );
    assert_error!(
        "@Let[h, @Handler[[n : i64] : i64, 0]];",
        ErrorKind::InvalidHandlerParam
    );
    assert_error!(
        "@Let[h, @Handler[[self : Pid[i64, string]] : i64, 0]];",
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn actor_primitives_require_handler_context() {
    assert_error!("@Let[x, @Continue[]];", ErrorKind::ActorPrimitiveOutsideHandler);
    assert_error!(
        "@Let[x, @Receive[0u, Option/None[]]];",
        ErrorKind::ActorPrimitiveOutsideHandler
    );
    assert_error!("@Let[x, @Halt[]];", ErrorKind::ActorPrimitiveOutsideHandler);
    assert_error!("@Let[x, @Sleep[1U]];", ErrorKind::ActorPrimitiveOutsideHandler);
    assert_error!(
        r"
@Let[_, @Handler[[self : Pid[i64, i64]] : i64,
  @Match[@Monitor[self], {
    (/Some[id], id),
    (/None[], 0)
  }]]];
",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        "@Main[[self : Pid[nil, nil]] : nil, @Block[nil, @Continue[]]]",
        ErrorKind::ContinueInMain
    );
    assert_error!(
        "@Main[[self : Pid[nil, nil]] : nil, @Halt[]]",
        ErrorKind::HaltInMain
    );
    assert_error!(
        r"
@Main[[self : Pid[nil, nil]] : nil,
@Block[@Let[p, @Spawn[5]], nil]]
",
        ErrorKind::NotHandler { .. }
    );
    assert_error!(
        r#"
@Main[[self : Pid[nil, nil]] : nil,
@Block[
    @Let[h, @Handler[[self : Pid[i64, string]] : string, "ok"]],
    @Let[p, @Spawn[h]],
    @Let[x, @Send[p, "i", 1]]
]]
"#,
        ErrorKind::U8LiteralRequired
    );
    assert_error!(
        r#"
@Main[[self : Pid[nil, nil]] : nil,
@Block[
    @Let[h, @Handler[[self : Pid[i64, string]] : string, "ok"]],
    @Let[p, @Spawn[h]],
    @Let[x, @Send[p, 1u, 1]]
]]
"#,
        ErrorKind::MailboxIndexOutOfRange { .. }
    );
    assert_error!(
        r"
@Main[[self : Pid[nil, nil]] : nil,
@Block[
    @Let[h, @Handler[[self : Pid[i64, i64]] : i64,
      @Match[@Receive[0u, Option/None[]], {
        (/Some[x], x),
        (/None[], 0)
      }]]],
    @Let[p, @Spawn[h]]
]]
"
        .replace("0u", "0"),
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn main_signature_is_fixed() {
    assert_error!(
        "@Main[[self : Pid[nil, nil], extra : i64] : nil, nil]",
        ErrorKind::InvalidMainParam
    );
    assert_error!(
        "@Main[[self : Pid[nil, nil]] : i64, 1]",
        ErrorKind::InvalidMainReturn
    );
}

#[test]
fn values_must_be_used_and_determined() {
    assert_error!("5;", ErrorKind::UnusedValue { .. });
    assert_error!("@Block[5, nil];", ErrorKind::UnusedValue { .. });
    assert_error!("@Let[_, {}];", ErrorKind::UndeterminedType { .. });
    assert_error!("@Let[_, Option/None[]];", ErrorKind::UndeterminedType { .. });
    assert_error!(
        "@Let[_, {Option/None[], Option/None[]}];",
        ErrorKind::UndeterminedType { .. }
    );
    assert_error!("@Let[_, {1, 2u}];", ErrorKind::TypeMismatch { .. });
}

#[test]
fn format_specs_are_checked() {
    assert_error!(r#"@Let[x, f"{1:?}"];"#, ErrorKind::TypeMismatch { .. });
    assert_error!(r#"@Let[x, f"{2u:.2}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{2U:#.8x}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{1:w}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{1:e}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{1.5:x}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{1.0:#}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{8:#o}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{1:257}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{1.0:.25}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(
        r#"@Let[x, 1]; @Let[_, f"{x:.2}"];"#,
        ErrorKind::InvalidFormatSpec { .. }
    );
    assert_error!(
        r#"@Let[d : Dict[string, i64], @Dict[]]; @Let[_, f"{d}"];"#,
        ErrorKind::UnprintableType { .. }
    );
    assert_error!(
        r#"@Data[D, {C[]}]; @Let[v, D/C[]]; @Let[_, f"{v}"];"#,
        ErrorKind::UnprintableType { .. }
    );
    assert_error!(
        r#"@Let[o : Option[i64], Option/Some[1]]; @Let[_, f"{o}"];"#,
        ErrorKind::UnprintableType { .. }
    );
    assert_error!(
        r#"@Let[r, OpenRead["x"]]; @Let[_, f"{r}"];"#,
        ErrorKind::UnprintableType { .. }
    );
}

#[test]
fn container_keys_are_restricted() {
    assert_error!(
        "@Let[_ : Dict[f64, i64], @Dict[]];",
        ErrorKind::InvalidKeyType { .. }
    );
    assert_error!("@Set[1.5];", ErrorKind::InvalidKeyType { .. });
}

#[test]
fn option_and_result_helpers_check_element_types() {
    assert_error!(
        r"
@Let[o : Option[i64], Option/Some[1]];
@Let[m, @OptionMap[o, @Fn[[x : string] : i64, 1]]];
",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        "@Let[o : Option[i64], Option/None[]]; @Let[x, @OptionGet[o, \"s\"]];",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        r"
@Let[o : Option[i64], Option/Some[1]];
@Let[m, @OptionAndThen[o, @Fn[[x : i64] : i64, x]]];
",
        ErrorKind::InferMismatch { .. }
    );
    assert_error!("@Let[x, @IsSome[]];", ErrorKind::ArityMismatch { .. });
    assert_error!(
        "@Let[o : Option[i64], Option/Some[1]]; @Let[x, @IsSome[o, o]];",
        ErrorKind::ArityMismatch { .. }
    );
    assert_error!(
        "@Let[o : Option[i64], Option/Some[1]]; @Let[x, @OptionMap[o]];",
        ErrorKind::ArityMismatch { .. }
    );
}

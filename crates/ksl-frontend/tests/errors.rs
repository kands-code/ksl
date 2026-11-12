//! 前端错误的种类断言.
//!
//! 错误编号由枚举顺序生成, 不是稳定契约; 本文件按 `ErrorKind` 断言错误语义.

use ksl_diagnostics::error::{ErrorKind, KslError};
use ksl_frontend::check::Session;
use ksl_frontend::testutil::parse;
use ksl_frontend::validate::{SourceKind, validate};

fn error_with_kind(source: &str, source_kind: SourceKind) -> KslError {
    let program = match parse(source) {
        Ok(program) => program,
        Err(err) => return err,
    };
    if let Err(err) = validate(&program, source_kind) {
        return err;
    }
    let mut session = Session::new();
    session.check(&program).expect_err("脚本应报错")
}

fn error(source: &str) -> KslError {
    error_with_kind(source, SourceKind::Module)
}

macro_rules! assert_error {
    ($source:expr, $pattern:pat) => {{
        let error = error(&$source);
        assert!(
            matches!(error.kind(), Some(kind) if matches!(kind, $pattern)),
            "错误种类不符: {}\n源码: {}",
            error,
            $source
        );
    }};
}

#[test]
fn parse_errors_report_their_kind() {
    assert_error!("@Let[x, 1] x;", ErrorKind::UnexpectedToken);
    assert_error!("@Let[x, 1,];", ErrorKind::UnexpectedToken);
    assert_error!("@Let[x, 1", ErrorKind::Unterminated);
    assert_error!("@NoSuchForm[f];", ErrorKind::UnknownSpecialForm);
    assert_error!(r#"@Let[t, f"{1 + 2}"];"#, ErrorKind::InvalidFStrInterp);
    assert_error!("@Fix[@Let[f, 1]];", ErrorKind::ExpectedDef);
}

#[test]
fn nesting_is_limited_but_flat_sequences_are_not() {
    let flat = format!("@Let[_, {{{}}}];", vec!["1i64"; 500].join(", "));
    assert!(parse(&flat).is_ok(), "平铺的列表元素不计入嵌套");

    let mut deep_expr = "1i64".to_string();
    for _ in 0..200 {
        deep_expr = format!("AddI64[1i64, {deep_expr}]");
    }
    let source = format!("@Let[_, {deep_expr}];");
    assert_error!(source, ErrorKind::NestingTooDeep { .. });

    let mut deep_type = "i64".to_string();
    for _ in 0..200 {
        deep_type = format!("Option[{deep_type}]");
    }
    let source = format!("@Def[f, [x : {deep_type}] : i64, 0i64];");
    assert_error!(source, ErrorKind::NestingTooDeep { .. });

    let chain = format!("f{}", "[1i64]".repeat(200));
    let source = format!("@Let[_, {chain}];");
    assert_error!(source, ErrorKind::NestingTooDeep { .. });
}

#[test]
fn definition_and_exit_placements_are_checked() {
    assert_error!("@Block[@Data[X, {Y[]}]];", ErrorKind::DataNotTopLevel);
    assert_error!("@Block[@Export[A]];", ErrorKind::ModuleFormNotTopLevel);
    assert_error!(
        "@If[true, @Let[x, 1], @Let[y, 2]];",
        ErrorKind::DefinitionNotAllowed
    );
    assert_error!("@Def[f, [] : i64, @Exit[0u8]];", ErrorKind::ExitOutsideRoot);
}

#[test]
fn annotations_fix_literal_and_element_types() {
    assert_error!("@Let[x : u8, 300];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[x, AddU8[1, 2]];", ErrorKind::TypeMismatch { .. });
    assert_error!("@Let[l : List[u8], {1, 2}];", ErrorKind::TypeMismatch { .. });
    assert_error!(
        "@Let[p : Pair[u8, f64], (1, 2.0)];",
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn operations_check_against_annotations() {
    assert_error!(
        "@Let[x, @ListGet[{1, 2}, 0u8]];",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        r"
@Let[g : Fn[[i64] : bool], @Fn[[n : i64] : bool, true]];
@Let[y, @ListFilter[{'a'}, g]];
",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        r"
@Let[h : Fn[[i64, i64] : i64], @Fn[[a : i64, b : i64] : i64, a]];
@Let[y, @ListFold[{1, 2}, h, 1u8]];
",
        ErrorKind::TypeMismatch { .. }
    );
    assert_error!(
        "@Let[x : List[u8], @ListMap[{1, 2}, @Fn[[n : u8] : u8, n]]];",
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
    assert_error!("@Let[x, {1, 2u8}];", ErrorKind::TypeMismatch { .. });
}

#[test]
fn untyped_constructors_require_context() {
    assert_error!("@Let[op, Option/None[]];", ErrorKind::UndeterminedType { .. });
    assert_error!("@Let[r, Result/Ok[10]];", ErrorKind::UndeterminedType { .. });
    assert_error!("@Let[_, {}];", ErrorKind::UndeterminedType { .. });
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
fn which_checks_conditions_and_result_type() {
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
fn pattern_qualifier_errors_report_their_kind() {
    assert_error!(
        "@Data[Box, {Wrap[i64]}]; @Let[b, Box/Wrap[1]]; @Let[n, @Match[b, {(Typo/Wrap[v], \
         v)}]];",
        ErrorKind::UnknownType
    );
    assert_error!(
        "@Data[Box, {Wrap[i64]}]; @Let[b, Box/Wrap[1]]; @Match[b, {(Option/Some[v], v)}];",
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn data_get_requires_single_variant_and_literal_index() {
    assert_error!(
        "@Data[P, {I[u8]}]; @Let[p, P/I[1u8]]; @Let[x, @DataGet[1i64, 0u8]];",
        ErrorKind::NotData { .. }
    );
    assert_error!(
        "@Data[Q, {A[u8], B[u8]}]; @Let[x, @DataGet[Q/A[1u8], 0u8]];",
        ErrorKind::DataGetOnMultiVariant { .. }
    );
    assert_error!(
        "@Data[P, {I[u8]}]; @Let[p, P/I[1u8]]; @Let[x, @DataGet[p, 1u8]];",
        ErrorKind::FieldIndexOutOfRange { .. }
    );
    assert_error!(
        "@Data[P, {I[u8]}]; @Let[p, P/I[1u8]]; @Let[x, @DataGet[p, 0]];",
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn data_variant_field_count_is_limited() {
    let fields = vec!["u8"; 257].join(", ");
    let source = format!("@Data[P, {{I[{fields}]}}];");
    assert_error!(
        source.as_str(),
        ErrorKind::TooManyVariantFields { limit: 256, .. }
    );
}

#[test]
fn data_declarations_reject_duplicates_and_reserved_names() {
    assert_error!("@Data[D, {A[]}]; @Data[D, {B[]}];", ErrorKind::DuplicateData);
    assert_error!("@Data[D, {A[], A[i64]}];", ErrorKind::DuplicateVariant);
    assert_error!("@Data[Empty, {}];", ErrorKind::EmptyData);
    assert_error!("@Data[D, {D[]}];", ErrorKind::VariantNameConflict);
    assert_error!("@Data[List, {A[]}];", ErrorKind::ReservedName);
}

#[test]
fn names_must_resolve() {
    assert_error!("@Let[x, nosuch];", ErrorKind::UnknownName);
    assert_error!("@Let[x : Nope, 1];", ErrorKind::UnknownType);
}

#[test]
fn bindings_reject_reserved_and_duplicate_names() {
    assert_error!("@Let[AddI64, 5];", ErrorKind::ReservedName);
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
}

#[test]
fn calls_require_callable_and_matching_arity() {
    assert_error!(
        "@Let[value, 1]; @Let[_, value[2]];",
        ErrorKind::NotCallable { .. }
    );
    assert_error!(
        "@Let[x, @ListHead[{1, 2}, 0]];",
        ErrorKind::ArityMismatch { .. }
    );
}

#[test]
fn function_bodies_match_signatures() {
    assert_error!(
        "@Let[f, @Fn[[x : i64] : i64, \"s\"]];",
        ErrorKind::TypeMismatch { .. }
    );
}

#[test]
fn fix_definitions_are_scoped_to_their_block() {
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
}

#[test]
fn actor_primitives_require_handler_context() {
    assert_error!("@Let[x, @Continue[]];", ErrorKind::ActorPrimitiveOutsideHandler);
    assert_error!(
        "@Let[x, @Sleep[1u64]];",
        ErrorKind::ActorPrimitiveOutsideHandler
    );
    assert_error!(
        "@Main[[self : Pid[nil, nil]] : nil, @Block[nil, @Continue[]]]",
        ErrorKind::ContinueInMain
    );
    assert_error!(
        "@Main[[self : Pid[nil, nil]] : nil, @Halt[]]",
        ErrorKind::HaltInMain
    );
}

#[test]
fn spawn_and_send_check_their_arguments() {
    assert_error!(
        r"
@Main[[self : Pid[nil, nil]] : nil,
@Block[@Let[p, @Spawn[5]], nil]]
",
        ErrorKind::NotHandler { .. }
    );
    assert_error!(
        r"
@Main[[self : Pid[nil, nil]] : nil,
@Block[@Let[_, @Send[5, 0u8, 1]], nil]]
",
        ErrorKind::NotPid { .. }
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
    @Let[x, @Send[p, 1u8, 1]]
]]
"#,
        ErrorKind::MailboxIndexOutOfRange { .. }
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
fn unused_values_are_rejected() {
    assert_error!("5;", ErrorKind::UnusedValue { .. });
    assert_error!("@Block[5, nil];", ErrorKind::UnusedValue { .. });
}

#[test]
fn format_specs_are_checked() {
    assert_error!(r#"@Let[x, f"{true:?}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{2u8:.2}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(r#"@Let[x, f"{1:257}"];"#, ErrorKind::InvalidFormatSpec { .. });
    assert_error!(
        r#"@Let[x, 1]; @Let[_, f"{x:.2}"];"#,
        ErrorKind::InvalidFormatSpec { .. }
    );
    assert_error!(
        r#"@Let[d : Dict[string, i64], @Dict[]]; @Let[_, f"{d}"];"#,
        ErrorKind::UnprintableType { .. }
    );
    assert_error!(
        r#"@Let[f, @Fn[[x : i64] : i64, x]]; @Let[_, f"{f}"];"#,
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
}

#[test]
fn module_forms_require_module_source() {
    let code = SourceKind::Code;
    assert_eq!(
        error_with_kind("@Export[x];", code).kind(),
        Some(&ErrorKind::ExportOutsideModule)
    );
    assert_eq!(
        error_with_kind("@Main[[self : Pid[nil, nil]] : nil, nil]", code).kind(),
        Some(&ErrorKind::MainOutsideModule)
    );
    assert!(matches!(
        error_with_kind(r#"@Link["x", { }, { f : Fn[[] : nil] }];"#, code).kind(),
        Some(&ErrorKind::LinkOutsideModule)
    ));
    assert_eq!(
        error_with_kind("@Let[x, GetModuleDirectory[]];", code).kind(),
        Some(&ErrorKind::ModuleDirectoryOutsideModule)
    );
}

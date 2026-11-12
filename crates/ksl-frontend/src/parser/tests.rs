//! parser 模块测试.

use super::*;
use crate::ast::Special;

fn parse_str(source: &str) -> Result<Program, KslError> {
    crate::testutil::parse(source)
}

fn error_kind(source: &str) -> ErrorKind {
    parse_str(source).expect_err("源码应被拒绝").kind
}

#[test]
fn top_level_statements_and_tail_expression() {
    let program = parse_str("@Let[x : i64, 12]; @Let[y, 13];").unwrap();
    assert_eq!(program.stmts.len(), 2);
    assert!(program.tail.is_none());

    let program = parse_str("@Let[x, 1]; x").unwrap();
    assert_eq!(program.stmts.len(), 1);
    assert!(program.tail.is_some());

    assert_eq!(error_kind("@Let[x, 1] x;"), ErrorKind::UnexpectedToken);
    assert_eq!(error_kind("@Let[x, 1"), ErrorKind::Unterminated);
}

#[test]
fn functions_lists_and_pairs() {
    assert!(
        parse_str("@Let[f : Fn[[i64] : i64], @Fn[[x : i64] : i64, AddI64[x, 1]]];").is_ok()
    );
    assert!(parse_str("@Let[a : List[char], {'a', 'b'}]; @Let[p, (1, 'a')];").is_ok());

    let source = r"
@Data[Shape, {Point[], Circle[f64], Rectangle[f64, f64]}];
@Match[p, {(Shape/Circle[r], r), (/Point[], 0.0)}];
";
    assert_eq!(parse_str(source).unwrap().stmts.len(), 2);
}

#[test]
fn named_functions_and_fix() {
    let source = r"
@Def[addOne, [x : i64] : i64, AddI64[x, 1]];
@Fix[
  @Def[isEven, [n : u8] : bool, true],
  @Def[isOdd, [n : u8] : bool, false]
];
";
    assert!(parse_str(source).is_ok());
}

#[test]
fn module_declarations() {
    let source = r#"
@Export[A, f, ^Shape];
@Use[{A, (f, g), ^Shape, ^(S, T)}, "lib/a"];
@Let[x, 1];
x
"#;
    let program = parse_str(source).unwrap();
    assert_eq!(program.stmts.len(), 3);

    let ExprKind::Special(Special::Export { items }) = &program.stmts[0].node else {
        panic!("应为 @Export");
    };
    assert_eq!(items.len(), 3);

    let ExprKind::Special(Special::Use { items, path }) = &program.stmts[1].node else {
        panic!("应为 @Use");
    };
    assert_eq!(path, "lib/a");
    assert_eq!(items.len(), 4);

    let source = r#"@Link["ray", { ^Texture, ^Font }, { f : Fn[[Texture] : Font], v : i64 }];"#;
    let program = parse_str(source).unwrap();
    let ExprKind::Special(Special::Link(link)) = &program.stmts[0].node else {
        panic!("应为 @Link");
    };
    assert_eq!(link.plugin, "ray");
    assert_eq!(link.types.len(), 2);
    assert_eq!(link.values.len(), 2);
}

#[test]
fn fstring_interpolations_accept_variables_and_literals() {
    assert!(parse_str(r#"@Let[s, f"{x}"]; "#).is_ok());
    assert!(parse_str(r#"@Let[s, f"{nil}"]; "#).is_ok());
    assert!(parse_str(r#"@Let[s, f"{':'} {"a:b"} {r"x}y"}"]; "#).is_ok());
    assert!(parse_str(r#"@Let[s, f"{AddI64[1, 2]}"]; "#).is_err());
}

#[test]
fn declarations_are_restricted_to_top_level() {
    assert_eq!(
        error_kind("@Block[@Data[X, {Y[]}], 1];"),
        ErrorKind::DataNotTopLevel
    );
    assert_eq!(
        error_kind("@Let[f, @Def[x, [y : i64] : i64, y]];"),
        ErrorKind::DefinitionNotAllowed
    );
    assert_eq!(
        error_kind("@If[true, @Let[x, 1], @Let[y, 2]];"),
        ErrorKind::DefinitionNotAllowed
    );
    assert_eq!(error_kind("@Fix[@Let[f, 1]];"), ErrorKind::ExpectedDef);
    assert_eq!(
        error_kind("@Block[@Export[A]];"),
        ErrorKind::ModuleFormNotTopLevel
    );
    assert!(parse_str("@Export[A]").is_ok());
    assert!(parse_str(r#"@Use[{A}, "lib/a"]"#).is_ok());
}

#[test]
fn exit_is_restricted_to_top_level_and_main() {
    assert!(parse_str("@Exit[3u];").is_ok());
    assert!(parse_str("@Exit[3u]").is_ok());
    assert_eq!(error_kind("@Block[@Exit[3u]];"), ErrorKind::ExitNotTopLevel);
    assert_eq!(error_kind("@Let[x, @Exit[3u]];"), ErrorKind::ExitNotTopLevel);
    assert_eq!(
        error_kind("@If[true, @Exit[3u], 0];"),
        ErrorKind::ExitNotTopLevel
    );
    assert!(parse_str("@Main[[self : Pid[nil, nil]] : nil, @Block[@Exit[0u]]];").is_ok());
    assert_eq!(
        error_kind(
            "@Main[[self : Pid[nil, nil]] : nil, @Block[@Let[h, @Handler[[self : Pid[nil, \
             nil]] : nil, @Exit[0u]]], nil]];"
        ),
        ErrorKind::ExitNotTopLevel
    );
}

#[test]
fn handler_and_continue_forms_parse() {
    assert!(
        parse_str(r#"@Let[h, @Handler[[self : Pid[i64, string], n : i64] : string, "ok"]];"#)
            .is_ok()
    );
    assert!(parse_str("@Continue[1];").is_ok());
}

#[test]
fn rejects_invalid_syntax() {
    assert_eq!(
        parse_str("@NoSuchForm[f];").unwrap_err().kind,
        ErrorKind::UnknownSpecialForm
    );
    for source in [
        "@Let[x, 1,];",
        "@If[true, 1, 2,];",
        "{1, 2,};",
        "@Data[D, {C[i64,]}];",
        "@Data[D, {A[], B[i64],}];",
        "@Block[1,];",
        "@Fix[@Def[f, [] : i64, 1],];",
        "@Spawn[h,];",
        "@ListMap[xs, f,];",
        "(1, 2,);",
    ] {
        assert_eq!(
            parse_str(source).unwrap_err().kind,
            ErrorKind::UnexpectedToken,
            "{source}"
        );
    }
}

#[test]
fn rejects_invalid_fstr_interpolation() {
    for source in [r#"@Let[t, f"{1 + 2}"];"#, r#"@Let[f, 1]; @Let[t, f"{f[0]}"];"#] {
        assert_eq!(
            parse_str(source).unwrap_err().kind,
            ErrorKind::InvalidFStrInterp,
            "{source}"
        );
    }
}

#[test]
fn fstr_interp_error_uses_interp_span() {
    let source = r#"@Let[t, f"abc {1 + 2} def"];"#;
    let error = parse_str(source).unwrap_err();
    let span = error.span.expect("插值错误应带位置");
    assert_eq!(&source[span.start..span.end], "1 + 2");
}

#[test]
fn every_special_form_name_dispatches() {
    for name in crate::ast::special_form_names() {
        let source = format!("@{name}[]");
        if let Err(err) = parse_str(&source) {
            assert_ne!(
                err.kind,
                ErrorKind::UnknownSpecialForm,
                "{name} 登记在形式表中却没有分派"
            );
        }
    }
}

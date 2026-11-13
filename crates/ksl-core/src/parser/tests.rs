//! parser 模块测试.

use super::*;
use crate::ast::Special;

fn parse_str(source: &str) -> Result<Program, KslError> {
    crate::testutil::parse(source)
}

#[test]
fn parses_language_forms() {
    // 顶层语句以分号结尾; 最后一条表达式可省略分号作尾表达式.
    let program = parse_str("@Let[x : i64, 12]; @Let[y, 13];").unwrap();
    assert_eq!(program.stmts.len(), 2);
    assert!(program.tail.is_none());
    let program = parse_str("@Let[x, 1]; x").unwrap();
    assert_eq!(program.stmts.len(), 1);
    assert!(program.tail.is_some());
    assert_eq!(
        parse_str("@Let[x, 1] x;").unwrap_err().kind,
        ErrorKind::UnexpectedToken
    );

    // 匿名函数, Data 匹配, 列表与配对.
    assert!(
        parse_str("@Let[f : Fn[[i64] : i64], @Fn[[x : i64] : i64, AddI64[x, 1]]];").is_ok()
    );
    let source = r"
        @Data[Shape, {Point[], Circle[f64], Rectangle[f64, f64]}];
        @Match[p, {(Shape/Circle[r], r), (/Point[], 0.0)}];
    ";
    assert_eq!(parse_str(source).unwrap().stmts.len(), 2);
    assert!(parse_str("@Let[a : List[char], {'a', 'b'}]; @Let[p, (1, 'a')];").is_ok());

    // 具名函数与 @Fix.
    let source = r"
        @Def[addOne, [x : i64] : i64, AddI64[x, 1]];
        @Fix[
          @Def[isEven, [n : u8] : bool, true],
          @Def[isOdd, [n : u8] : bool, false]
        ];
    ";
    assert!(parse_str(source).is_ok());

    // 模块声明.
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

    // 插值内只允许变量或字面值.
    assert!(parse_str(r#"@Let[s, f"{x}"]; "#).is_ok());
    assert!(parse_str(r#"@Let[s, f"{nil}"]; "#).is_ok());
    assert!(parse_str(r#"@Let[s, f"{AddI64[1, 2]}"]; "#).is_err());
    assert!(parse_str(r#"@Let[s, f"{':'} {"a:b"} {r"x}y"}"]; "#).is_ok());
}

#[test]
fn rejects_invalid_positions() {
    assert_eq!(
        parse_str("@Block[@Data[X, {Y[]}], 1];").unwrap_err().kind,
        ErrorKind::DataNotTopLevel
    );
    assert_eq!(
        parse_str("@Let[f, @Def[x, [y : i64] : i64, y]];")
            .unwrap_err()
            .kind,
        ErrorKind::DefinitionNotAllowed
    );
    assert_eq!(
        parse_str("@If[true, @Let[x, 1], @Let[y, 2]];")
            .unwrap_err()
            .kind,
        ErrorKind::DefinitionNotAllowed
    );
    assert_eq!(
        parse_str("@Fix[@Let[f, 1]];").unwrap_err().kind,
        ErrorKind::ExpectedDef
    );
    // Exit 是顶层形式, 可以省略最后的分号.
    assert!(parse_str("@Exit[3u];").is_ok());
    assert!(parse_str("@Exit[3u]").is_ok());
    assert_eq!(
        parse_str("@Block[@Exit[3u]];").unwrap_err().kind,
        ErrorKind::ExitNotTopLevel
    );
    assert_eq!(
        parse_str("@Let[x, @Exit[3u]];").unwrap_err().kind,
        ErrorKind::ExitNotTopLevel
    );
    assert_eq!(
        parse_str("@If[true, @Exit[3u], 0];").unwrap_err().kind,
        ErrorKind::ExitNotTopLevel
    );
    // Handler 与 Continue 的形态.
    assert!(
        parse_str(r#"@Let[h, @Handler[[self : Pid[i64, string], n : i64] : string, "ok"]];"#)
            .is_ok()
    );
    assert!(parse_str("@Continue[1];").is_ok());

    // 模块声明只在顶层语句位置成立.
    assert_eq!(
        parse_str("@Block[@Export[A]];").unwrap_err().kind,
        ErrorKind::ModuleFormNotTopLevel
    );
    assert!(parse_str("@Export[A]").is_ok());
    assert!(parse_str(r#"@Use[{A}, "lib/a"]"#).is_ok());
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

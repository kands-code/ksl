//! builtin 模块测试.

use std::collections::HashSet;

use ksl_catalog::builtin;
use ksl_frontend::check::Type;

use super::*;

/// 目录文档中登记的条目.
struct CatalogEntry {
    name: String,
    params: Vec<String>,
    ret: String,
}

/// 类型转为 ksl 风格的类型记号.
fn type_text(ty: &Type) -> String {
    match ty {
        Type::I64 => "i64".into(),
        Type::U8 => "u8".into(),
        Type::U64 => "u64".into(),
        Type::F64 => "f64".into(),
        Type::Char => "char".into(),
        Type::Bool => "bool".into(),
        Type::Str => "string".into(),
        Type::Nil => "nil".into(),
        Type::Never => "never".into(),
        Type::Fn(params, ret) => {
            let args: Vec<String> = params.iter().map(type_text).collect();
            format!("Fn[[{}] : {}]", args.join(", "), type_text(ret))
        },
        Type::App { name, args } => {
            let name = ksl_base::symbol::resolve(*name);
            if args.is_empty() {
                name.to_string()
            } else {
                let inner: Vec<String> = args.iter().map(type_text).collect();
                format!("{name}[{}]", inner.join(", "))
            }
        },
        Type::Opaque(name) => ksl_base::symbol::resolve(*name).to_string(),
    }
}

/// 去掉类型文本中的空白, 便于比较.
fn normalize_type(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 按顶层分隔符切分; 方括号与圆括号内部的符号不切分.
fn split_top_level(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, c) in text.char_indices() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if c == separator && depth == 0 => {
                parts.push(text[start..index].trim());
                start = index + c.len_utf8();
            },
            _ => {},
        }
    }
    parts.push(text[start..].trim());
    parts
}

/// 去掉参数名, 只保留类型.
fn parameter_type(parameter: &str) -> String {
    let mut depth = 0usize;
    for (index, c) in parameter.char_indices() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                return parameter[index + 1..].trim().to_owned();
            },
            _ => {},
        }
    }
    parameter.trim().to_owned()
}

/// 解析 `名字[参数 : 类型, ...] : 返回类型`.
fn parse_named_signature(text: &str) -> Option<(String, Vec<String>, String)> {
    let open = text.find('[')?;
    let mut depth = 0usize;
    let mut close = None;
    for (offset, c) in text[open..].char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            },
            _ => {},
        }
    }
    let close = close?;
    let name = text[..open].trim().to_owned();
    let rest = text[close + 1..].trim().strip_prefix(':')?.trim();
    let params = split_top_level(&text[open + 1..close], ',')
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(parameter_type)
        .collect();
    Some((name, params, rest.to_owned()))
}

/// 读取 `docs/topics/library` 下所有 Markdown 文件中的签名条目.
fn catalog_entries() -> Vec<CatalogEntry> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/topics/library");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("目录文档缺失")
        .filter_map(|entry| {
            let path = entry.expect("目录项不可读").path();
            (path.extension().is_some_and(|ext| ext == "md")).then_some(path)
        })
        .collect();
    files.sort();
    let mut entries = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).expect("目录文档不可读");
        for line in text.lines() {
            let Some(rest) = line.strip_prefix("- `") else {
                continue;
            };
            let Some((head, _)) = rest.split_once('`') else {
                continue;
            };
            if let Some((name, params, ret)) = parse_named_signature(head) {
                entries.push(CatalogEntry { name, params, ret });
            }
        }
    }
    entries
}

/// 全部特殊形式名, 带 `@` 前缀.
///
/// 名单由形式表与操作元数据给出, 新增形式无须在此登记.
fn special_form_names() -> Vec<String> {
    let mut names: Vec<String> = ksl_frontend::ast::special_form_names()
        .map(|name| format!("@{name}"))
        .collect();
    names.extend(ksl_frontend::ast::operation_names().map(|name| format!("@{name}")));
    names
}

#[test]
fn builtin_ids_are_unique_and_round_trip() {
    // 注册表是检查器与运行时的唯一来源; 编号与符号必须一一对应.
    let mut seen = HashSet::new();
    for (index, entry) in builtin::specs().enumerate() {
        assert!(seen.insert(entry.symbol), "重复内置名: {}", entry.name);
        let expected = u16::try_from(index).expect("内置函数数量超出 u16");
        assert_eq!(super::id(entry.symbol).map(BuiltinId::index), Some(expected));
        assert_eq!(super::symbol(BuiltinId::from_index(expected)), entry.symbol);
        assert_eq!(super::name(BuiltinId::from_index(expected)), entry.name);
        assert!(super::signature(entry.symbol).is_some());
    }
}

#[test]
fn catalog_matches_registry() {
    // 目录文档的名字与签名须与注册表一致; 参数名只影响文档, 比较时只看类型.
    let entries = catalog_entries();
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    for entry in builtin::specs() {
        let name = entry.name;
        let signature = super::signature(entry.symbol).expect("注册表条目缺签名");
        let Type::Fn(params, ret) = signature else {
            panic!("内置函数签名应为 Fn: {name}");
        };
        let expected_params: Vec<String> = params.iter().map(type_text).collect();
        let expected_ret = type_text(ret);
        let Some(document) = entries.iter().find(|document| document.name == name) else {
            missing.push(name);
            continue;
        };
        let actual_params: Vec<String> = document
            .params
            .iter()
            .map(|param| normalize_type(param))
            .collect();
        let expected_params_normalized: Vec<String> = expected_params
            .iter()
            .map(|param| normalize_type(param))
            .collect();
        if actual_params != expected_params_normalized
            || normalize_type(&document.ret) != normalize_type(&expected_ret)
        {
            mismatched.push(format!(
                "{name}: 文档 {} : {}, 实现 {} : {}",
                document.params.join(", "),
                document.ret,
                expected_params.join(", "),
                expected_ret
            ));
        }
    }
    missing.sort_unstable();
    mismatched.sort();
    assert!(missing.is_empty(), "内置函数未进目录文档: {missing:?}");
    assert!(mismatched.is_empty(), "目录类型与实现不一致: {mismatched:?}");

    let mut known: HashSet<String> =
        builtin::specs().map(|entry| entry.name.to_owned()).collect();
    known.extend(special_form_names());
    let mut unknown: Vec<String> = entries
        .iter()
        .map(|entry| entry.name.clone())
        .filter(|name| !known.contains(name))
        .collect();
    unknown.sort();
    assert!(unknown.is_empty(), "目录文档写入了不存在的名字: {unknown:?}");

    let documented: HashSet<String> = entries.into_iter().map(|entry| entry.name).collect();
    let mut absent: Vec<String> = special_form_names()
        .into_iter()
        .filter(|name| !documented.contains(name))
        .collect();
    absent.sort();
    assert!(absent.is_empty(), "特殊形式未进目录文档: {absent:?}");
}

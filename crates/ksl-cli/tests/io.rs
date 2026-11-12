//! 标准 I/O 端到端测试.

use common::{assert_stdout_contains, run, run_in, run_with_input};
use ksl_base::testutil::TempDir;

mod common;

fn temp_root() -> TempDir {
    TempDir::new("ksl_cli_io_")
}

#[test]
fn read_line_handles_input_and_eof() {
    let read = r#"
Print["before\n"];
@Match[ReadLine["> "], {
  (/Ok[result], @Match[result, {
    (/Some[line], Print[f"{line}\n"]),
    (/None[], Print["eof\n"])
  }]),
  (/Err[_], nil)
}]
"#;
    let output = run_with_input(&["run", "--code", read], "hello\n");
    assert_stdout_contains(&output, "before\n> hello");
    let output = run_with_input(&["run", "--code", read], "");
    assert_stdout_contains(&output, "> eof");
}

#[test]
fn binary_file_round_trip() {
    let root = temp_root();
    let path = root.path().join("bytes.bin");
    let path = path.to_string_lossy();
    let invalid = root.path().join("invalid.bin");
    let invalid = invalid.to_string_lossy();
    let code = format!(
        r#"
@Let[_, @Match[WriteFileBytes["{path}", {{65u, 66u, 67u}}], {{
  (/Ok[v], v),
  (/Err[e], nil)
}}]];
@Let[bytes, @Match[ReadFileBytes["{path}"], {{
  (/Ok[b], b),
  (/Err[e], {{}})
}}]];
@Let[text, @Match[ReadFile["{path}"], {{
  (/Ok[s], s),
  (/Err[e], "")
}}]];
@Let[_, @Match[WriteFileBytes["{invalid}", {{255u}}], {{
  (/Ok[v], v),
  (/Err[e], nil)
}}]];
@Let[bad, @Match[ReadFile["{invalid}"], {{
  (/Ok[s], s),
  (/Err[e], @Match[e, {{
    (FileError/InvalidUtf8[p], "invalid"),
    (_, "other")
  }}])
}}]];
@Let[n, @ListLength[bytes]];
Print[f"{{n}} {{text}} {{bad}}\\n"]
"#
    );
    let output = run(&["run", "--code", &code]);
    assert_stdout_contains(&output, "3U ABC invalid");
}

#[test]
fn append_and_copy_files() {
    let root = temp_root();
    let code = r#"
@Let[_, @Match[WriteFile["a.txt", "A"], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[_, @Match[AppendFile["a.txt", "B"], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[text, @Match[ReadFile["a.txt"], {
  (/Ok[s], s),
  (/Err[e], "")
}]];
@Let[_, @Match[WriteFileBytes["b.bin", {65u}], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[_, @Match[AppendFileBytes["b.bin", {66u, 67u}], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[bytes, @Match[ReadFileBytes["b.bin"], {
  (/Ok[b], b),
  (/Err[e], {})
}]];
@Let[_, @Match[AppendFile["created.txt", "C"], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[created, @Match[ReadFile["created.txt"], {
  (/Ok[s], s),
  (/Err[e], "")
}]];
@Let[_, @Match[CopyFile["a.txt", "c.txt"], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[copied, @Match[ReadFile["c.txt"], {
  (/Ok[s], s),
  (/Err[e], "")
}]];
@Let[n, @ListLength[bytes]];
Print[f"{text} {created} {copied}\n"];
Print[f"{n}\n"]
"#;
    let output = run_in(root.path(), &["run", "--code", code], "");
    assert_stdout_contains(&output, "AB C AB");
    assert_stdout_contains(&output, "3U");
}

#[test]
fn rename_path_updates_location() {
    let root = temp_root();
    let code = r#"
@Let[_, @Match[WriteFile["c.txt", "text"], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[_, @Match[RenamePath["c.txt", "d.txt"], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[old_exists, IsPathExists["c.txt"]];
@Let[new_text, @Match[ReadFile["d.txt"], {
  (/Ok[s], s),
  (/Err[e], "")
}]];
@Let[missing_rename, @Match[RenamePath["missing.txt", "x.txt"], {
  (/Ok[v], "ok"),
  (/Err[e], @Match[e, {
    (FileError/NotFound[path], "notfound"),
    (_, "other")
  }])
}]];
Print[f"{old_exists} {new_text} {missing_rename}\n"]
"#;
    let output = run_in(root.path(), &["run", "--code", code], "");
    assert_stdout_contains(&output, "false text notfound");
}

#[test]
fn current_directory_resolves_relative_paths() {
    let root = temp_root();
    let code = r#"
@Let[_, @Match[WriteFile["a.txt", "A"], {
  (/Ok[v], v),
  (/Err[e], nil)
}]];
@Let[pwd, @Match[GetCurrentDirectory[], {
  (/Ok[s], s),
  (/Err[e], "")
}]];
@Let[pwd_ok, IsPathExists[JoinPath[pwd, "a.txt"]]];
Print[f"{pwd_ok}\n"]
"#;
    let output = run_in(root.path(), &["run", "--code", code], "");
    assert_stdout_contains(&output, "true");
}

#[test]
fn directory_operations_and_predicates() {
    let root = temp_root();
    let dir = root.path().join("work");
    let dir = dir.to_string_lossy();
    let missing = root.path().join("missing");
    let missing = missing.to_string_lossy();
    let code = format!(
        r#"
@Let[_, @Match[CreateDirectoryAll["{dir}"], {{
  (/Ok[v], v),
  (/Err[e], nil)
}}]];
@Let[file, JoinPath["{dir}", "a.bin"]];
@Let[_, @Match[WriteFileBytes[file, {{1u}}], {{
  (/Ok[v], v),
  (/Err[e], nil)
}}]];
@Let[names, @Match[ListDirectory["{dir}"], {{
  (/Ok[xs], xs),
  (/Err[e], {{}})
}}]];
@Let[is_dir, IsDirectory["{dir}"]];
@Let[not_file, IsFile["{dir}"]];
@Let[is_file, IsFile[file]];
@Let[missing_exists, IsPathExists["{missing}"]];
@Let[missing_file, IsFile["{missing}"]];
@Let[missing_dir, IsDirectory["{missing}"]];
@Let[n, @ListLength[names]];
@Let[_, @Match[RemoveFile[file], {{
  (/Ok[v], v),
  (/Err[e], nil)
}}]];
@Let[file_gone, IsPathExists[file]];
@Let[_, @Match[RemoveDirectory["{dir}"], {{
  (/Ok[v], v),
  (/Err[e], nil)
}}]];
@Let[dir_gone, IsPathExists["{dir}"]];
Print[f"{{n}} {{is_dir}} {{not_file}} {{is_file}}\\n"];
Print[f"{{missing_exists}} {{missing_file}} {{missing_dir}}\\n"];
Print[f"{{file_gone}} {{dir_gone}}\\n"]
"#
    );
    let output = run(&["run", "--code", &code]);
    assert_stdout_contains(&output, "1U true false true");
    assert_stdout_contains(&output, "false false false");
    assert_stdout_contains(&output, "false false");
}

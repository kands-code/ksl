//! 标准 I/O 端到端测试.

use assert_cmd::Command;
use common::TempDir;
use predicates::prelude::*;

mod common;

fn temp_root() -> TempDir {
    TempDir::new("ksl_cli_io_")
}

#[test]
fn read_line_handles_input_and_eof() {
    let read = r#"
        @Let[_p, Print["before"]];
        @Match[ReadLine["> "], {
          (/Ok[result], @Match[result, {
            (/Some[line], @Block[@Let[p, Print[line]], line]),
            (/None[], @Block[@Let[p, Print["eof"]], "eof"])
          }]),
          (/Err[e], "err")
        }]
    "#;
    Command::cargo_bin("ksl")
        .expect("命令行应可启动")
        .args(["run", "--code", read])
        .write_stdin("hello\n")
        .assert()
        .success()
        // 排队的输出必须在读取提示之前写出.
        .stdout(predicate::str::contains("before\n> hello"));
    Command::cargo_bin("ksl")
        .expect("命令行应可启动")
        .args(["run", "--code", read])
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::contains("eof"));
}

#[test]
fn read_line_plain_handles_input_and_eof() {
    let read = r#"
        @Match[ReadLinePlain["> "], {
          (/Ok[result], @Match[result, {
            (/Some[line], @Block[@Let[p, Print[line]], line]),
            (/None[], @Block[@Let[p, Print["eof"]], "eof"])
          }]),
          (/Err[e], "err")
        }]
    "#;
    Command::cargo_bin("ksl")
        .expect("命令行应可启动")
        .args(["run", "--code", read])
        .write_stdin("hello\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("> hello\n"));
    Command::cargo_bin("ksl")
        .expect("命令行应可启动")
        .args(["run", "--code", read])
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::contains("eof"));
}

#[test]
fn binary_file_round_trip() {
    let root = temp_root();
    let path = root.path().join("bytes.bin");
    let path = path.to_string_lossy();
    let code = format!(
        r#"
        @Let[_w, @Match[WriteFileBytes["{path}", {{65u, 66u, 67u}}], {{
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
        @Let[n, @ListLength[bytes]];
        Print[f"{{n}} {{text}}\\n"]
    "#
    );
    Command::cargo_bin("ksl")
        .expect("命令行应可启动")
        .args(["run", "--code", &code])
        .assert()
        .success()
        .stdout(predicate::str::contains("3U ABC"));
}

#[test]
fn append_copy_rename_and_current_directory() {
    let root = temp_root();
    let code = r#"
        @Let[_w, @Match[WriteFile["a.txt", "A"], {
          (/Ok[v], v),
          (/Err[e], nil)
        }]];
        @Let[_a, @Match[AppendFile["a.txt", "B"], {
          (/Ok[v], v),
          (/Err[e], nil)
        }]];
        @Let[text, @Match[ReadFile["a.txt"], {
          (/Ok[s], s),
          (/Err[e], "")
        }]];
        @Let[_wb, @Match[WriteFileBytes["b.bin", {65u}], {
          (/Ok[v], v),
          (/Err[e], nil)
        }]];
        @Let[_ab, @Match[AppendFileBytes["b.bin", {66u, 67u}], {
          (/Ok[v], v),
          (/Err[e], nil)
        }]];
        @Let[bytes, @Match[ReadFileBytes["b.bin"], {
          (/Ok[b], b),
          (/Err[e], {})
        }]];
        @Let[n, @ListLength[bytes]];
        @Let[_ac, @Match[AppendFile["created.txt", "C"], {
          (/Ok[v], v),
          (/Err[e], nil)
        }]];
        @Let[created, @Match[ReadFile["created.txt"], {
          (/Ok[s], s),
          (/Err[e], "")
        }]];
        @Let[_cp, @Match[CopyFile["a.txt", "c.txt"], {
          (/Ok[v], v),
          (/Err[e], nil)
        }]];
        @Let[copied, @Match[ReadFile["c.txt"], {
          (/Ok[s], s),
          (/Err[e], "")
        }]];
        @Let[_mv, @Match[RenamePath["c.txt", "d.txt"], {
          (/Ok[v], v),
          (/Err[e], nil)
        }]];
        @Let[old_exists, IsPathExists["c.txt"]];
        @Let[new_text, @Match[ReadFile["d.txt"], {
          (/Ok[s], s),
          (/Err[e], "")
        }]];
        @Let[pwd, @Match[GetCurrentDirectory[], {
          (/Ok[s], s),
          (/Err[e], "")
        }]];
        @Let[pwd_ok, IsPathExists[JoinPath[pwd, "a.txt"]]];
        @Let[missing_rename, @Match[RenamePath["missing.txt", "x.txt"], {
          (/Ok[v], "ok"),
          (/Err[e], @Match[e, {
            (IOError/NotFound[path], "notfound"),
            (_, "other")
          }])
        }]];
        Print[f"{text} {created} {copied} {new_text}\n"];
        Print[f"{n} {old_exists} {pwd_ok} {missing_rename}\n"]
    "#;
    Command::cargo_bin("ksl")
        .expect("命令行应可启动")
        .current_dir(root.path())
        .args(["run", "--code", code])
        .assert()
        .success()
        .stdout(predicate::str::contains("AB C AB AB"))
        .stdout(predicate::str::contains("3U false true notfound"));
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
        @Let[_d, @Match[CreateDirectoryAll["{dir}"], {{
          (/Ok[v], v),
          (/Err[e], nil)
        }}]];
        @Let[file, JoinPath["{dir}", "a.bin"]];
        @Let[_w, @Match[WriteFileBytes[file, {{1u}}], {{
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
        @Let[_rf, @Match[RemoveFile[file], {{
          (/Ok[v], v),
          (/Err[e], nil)
        }}]];
        @Let[file_gone, IsPathExists[file]];
        @Let[_rd, @Match[RemoveDirectory["{dir}"], {{
          (/Ok[v], v),
          (/Err[e], nil)
        }}]];
        @Let[dir_gone, IsPathExists["{dir}"]];
        Print[f"{{n}} {{is_dir}} {{not_file}} {{is_file}}\\n"];
        Print[f"{{missing_exists}} {{missing_file}} {{missing_dir}}\\n"];
        Print[f"{{file_gone}} {{dir_gone}}\\n"]
    "#
    );
    Command::cargo_bin("ksl")
        .expect("命令行应可启动")
        .args(["run", "--code", &code])
        .assert()
        .success()
        .stdout(predicate::str::contains("1U true false true"))
        .stdout(predicate::str::contains("false false false"))
        .stdout(predicate::str::contains("false false"));
}

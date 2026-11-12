//! 运行时文件 I/O.

use ksl_value::value::Value;

use super::{run, run_with};
use crate::runtime::scheduler::Scheduler;
use crate::testutil::TempDir;

fn run_text(source: &str) -> String {
    match run(source) {
        Ok(Value::Str(text)) => text.to_string(),
        Ok(other) => panic!("结果不是字符串: {other:?}"),
        Err(error) => panic!("求值失败: {error:?}"),
    }
}

fn run_int(source: &str) -> i64 {
    match run(source) {
        Ok(Value::I64(value)) => value,
        Ok(other) => panic!("结果不是整数: {other:?}"),
        Err(error) => panic!("求值失败: {error:?}"),
    }
}

#[test]
fn streaming_reads_stop_at_end() {
    let dir = TempDir::new("ksl_io_streams_");

    let text_path = dir.path().join("stream.txt");
    let text_path = text_path.to_str().expect("路径应为 UTF-8");
    let text_source = format!(
        r#"
@Let[w, @ResultGet[OpenWrite["{text_path}"], @Panic["打开写入失败"]]];
@Let[_, WriteString[w, "a\nb\n"]];
@Let[_, Close[w]];
@Let[r, @ResultGet[OpenRead["{text_path}"], @Panic["打开读取失败"]]];
@Let[l1, @OptionGet[@ResultGet[ReadLineFrom[r], @Panic["读取失败"]], ""]];
@Let[l2, @OptionGet[@ResultGet[ReadLineFrom[r], @Panic["读取失败"]], ""]];
@Let[_, Close[r]];
f"{{l1}}{{l2}}"
"#
    );
    assert_eq!(run_text(&text_source), "ab");

    let chunk_path = dir.path().join("chunks.bin");
    let chunk_path = chunk_path.to_str().expect("路径应为 UTF-8");
    let chunk_source = format!(
        r#"
@Let[w, @ResultGet[OpenWrite["{chunk_path}"], @Panic["打开写入失败"]]];
@Let[_, WriteBytes[w, {{65u8, 66u8, 10u8}}]];
@Let[_, Close[w]];
@Let[r, @ResultGet[OpenRead["{chunk_path}"], @Panic["打开读取失败"]]];
@Let[n1, @ListLength[@OptionGet[@ResultGet[ReadChunk[r, 2u64], @Panic["读取失败"]], {{}}]]];
@Let[n2, @ListLength[@OptionGet[@ResultGet[ReadChunk[r, 2u64], @Panic["读取失败"]], {{}}]]];
@Let[n3, @ListLength[@OptionGet[@ResultGet[ReadChunk[r, 2u64], @Panic["读取失败"]], {{}}]]];
@Let[_, Close[r]];
AddI64[AddI64[U64ToI64[n1], U64ToI64[n2]], U64ToI64[n3]]
"#
    );
    assert_eq!(run_int(&chunk_source), 3);

    if cfg!(target_pointer_width = "64") {
        let huge_path = dir.path().join("huge.bin");
        std::fs::write(&huge_path, b"abc").expect("测试文件应可写入");
        let huge_path = huge_path.to_str().expect("路径应为 UTF-8");
        let huge_source = format!(
            r#"
@Let[r, @ResultGet[OpenRead["{huge_path}"], @Panic["打开读取失败"]]];
@Let[chunk, @OptionGet[@ResultGet[ReadChunk[r, 1099511627776u64], @Panic["读取失败"]], {{}}]];
@Let[_, Close[r]];
U64ToI64[@ListLength[chunk]]
"#
        );
        assert_eq!(run_int(&huge_source), 3);
    }
}

#[test]
fn actor_io_resumes_after_completion() {
    let dir = TempDir::new("ksl_actor_io_");
    let path = dir.path().join("data.txt");
    let path = path.to_str().expect("路径应为 UTF-8");
    let source = format!(
        r#"
@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[written, WriteFile["{path}", "hello"]],
    @Match[written, {{
      (/Ok[n], @Block[
        @Let[reader, @Handler[
          [self : Pid[nil, Result[string, FileError]]] : Result[string, FileError],
          ReadFile["{path}"]]],
        @Let[p, @Spawn[reader]],
        @Let[text, @Match[@Join[p], {{
          (/Ok[result], @Match[result, {{
            (/Ok[text], text),
            (/Err[e], @Panic["读取失败"])
          }}]),
          (/Err[e], @Panic["读取 Actor 失败"])
        }}]],
        @Assert[EqualString[text, "hello"], "Actor I/O 应读到写入内容"]
      ]),
      (/Err[e], @Panic["写入失败"])
    }}]
  ]]
"#
    );
    let mut scheduler = Scheduler::new();
    let result = run_with(&mut scheduler, &source);
    assert!(matches!(result, Ok(Value::Nil)), "程序应正常结束: {result:?}");
    scheduler.shutdown();
}

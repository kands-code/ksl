---
updated: 2026-11-19
---

# ksl 插件 ABI

`@Link` 使用的动态库插件接口, 面向插件作者与运行时维护者.
语言层的 `@Link` 声明规则见 `docs/topics/language/modules.md` 第 2.1 节,
运行时架构见 `docs/topics/implementation/architecture.md` 第 10 节.

## 1. 插件形态

插件是一个动态库, 只导出一个入口 `ksl_plugin_init`.
入口返回静态描述符, 描述符列出不透明类型, 函数与全局值.
宿主在 `@Link` 声明所在模块的目录下查找 `plugin/` 子目录,
文件名为平台约定: macOS 为 `lib<名字>.dylib`, Linux 为 `lib<名字>.so`,
Windows 为 `<名字>.dll`.
路径规范化后必须位于模块根目录内, 与模块文件的规则一致;
链接阶段解析并固定该路径, 执行阶段才加载动态库.

描述符中的名字是 UTF-8 字符串, 带长度, 不要求 NUL 结尾.
描述符, 类型表, 名称字符串与函数指针都由插件静态持有,
在动态库卸载前必须保持有效.
C 头文件位于 `crates/ksl-plugin/include/ksl_plugin.h`, 与 Rust 定义布局一致.

## 2. 入口与描述符

```c
const KslPlugin *ksl_plugin_init(uint32_t host_abi_version);
```

宿主传入当前 ABI 版本. 插件不接受该版本时返回空指针.
描述符中的 `abi_version` 必须等于 `KSL_PLUGIN_ABI_VERSION`,
`name` 必须等于 `@Link` 中的插件名.
初始化与卸载入口可选, 字段为空表示插件没有对应入口.

```c
typedef struct {
  uint32_t abi_version;
  KslStr name;
  KslStr version;
  const KslOpaqueType *types;
  uint32_t type_count;
  const KslFunctionDef *functions;
  uint32_t function_count;
  const KslGlobalDef *globals;
  uint32_t global_count;
  KslInitEntry init;
  KslUnloadEntry unload;
  uint32_t unload_thread;
  KslEventPumpEntry pump;
} KslPlugin;
```

`KslStr` 是 `const uint8_t *ptr` 加 `size_t len`.
计数为 0 时对应指针可以为空; 计数非 0 时指针必须非空.

## 3. 类型

类型用标签加可选的不透明类型下标表示.

| 标签          | ksl 类型   | 值的负载                   |
| ------------- | ---------- | -------------------------- |
| `TYPE_I64`    | `i64`      | `int64_t`                  |
| `TYPE_U64`    | `u64`      | `uint64_t`                 |
| `TYPE_U8`     | `u8`       | `uint8_t`                  |
| `TYPE_F64`    | `f64`      | `double`                   |
| `TYPE_BOOL`   | `bool`     | `uint8_t`, 0 或 1          |
| `TYPE_CHAR`   | `char`     | `uint32_t`, Unicode 标量值 |
| `TYPE_STRING` | `string`   | `KslString`                |
| `TYPE_NIL`    | `nil`      | 无                         |
| `TYPE_OPAQUE` | 不透明类型 | `KslOpaqueValue`           |

字符串是 UTF-8 字节加长度, 允许内嵌 NUL.
不透明类型在插件类型表中按下标引用, 名字必须与 `@Link` 中声明的类型名一致.
不透明类型的 ksl 身份是 `$plugin/<插件文件的根相对路径>/<类型名>`, 跨模块共享.
同一份动态库共享身份, 不同文件即使同名也不共享;
导入方可以在自己的 `@Link` 签名中直接使用导入的 Link 类型,
前提是该声明解析到同一份动态库.
Link 值自动导出, 因此签名中使用的导入类型需要在该模块再次导出.

## 4. 值与调用

```c
typedef uint32_t (*KslFunctionEntry)(
  const KslValue *args, uint32_t argc, KslValue *out);
typedef uint32_t (*KslGlobalEntry)(KslValue *out);
```

返回状态分三种:

- `0` 成功; `out` 中是返回值.
- `1` 业务失败; `out` 中是 UTF-8 错误信息.
  声明返回 `Result[T, string]` 时, 宿主包成 `Result/Err[string]`.
- `2` 插件内部错误或 panic; 宿主报运行时错误.
  其他非 0 值也按内部错误处理.

函数描述包含 ksl 链接名, 插件源名, 参数类型数组, 参数个数, 标志, 返回类型, 入口与执行类别.
ksl 链接名对应 `@Link` 中使用的名字; 插件源名对应 Rust 函数名, 供诊断与工具使用.
`#[ksl_export("Greet")]` 只改链接名.
标志含 `FUNCTION_FALLIBLE` 表示函数返回 `Result[T, string]`;
宿主会拒绝该标志与 `@Link` 声明不一致的函数.
执行类别取 `EXECUTION_CURRENT`, `EXECUTION_IO` 或 `EXECUTION_CPU`:

| 类别                | 执行位置   | 契约                                 |
| ------------------- | ---------- | ------------------------------------ |
| `EXECUTION_CURRENT` | 调度线程   | 入口很快返回, 不等待外部事件         |
| `EXECUTION_IO`      | I/O 线程池 | 入口阻塞在外部事件上, Actor 挂起等待 |
| `EXECUTION_CPU`     | CPU 线程池 | 入口做长计算, Actor 挂起等待         |

I/O 与 CPU 使用独立线程池, 长计算不会挤占文件与子进程.
两类调用都先让当前任务让出, 完成后经 I/O 完成事件恢复.
`EXECUTION_CURRENT` 调用在调度线程上同步完成, 不转移到线程池.

全局值描述包含 ksl 链接名, 插件源名, 类型, getter 与读取类别.
`#[ksl_global("TITLE")]` 同样保留 Rust 常量或静态项名作为源名.
读取类别只支持 `EXECUTION_CURRENT`; 其余类别在加载时拒绝.
全局值在模块执行时读取一次并绑定为 ksl 值, 之后不再调用 getter.

### 4.1 可恢复失败

可恢复失败统一用 `Result[T, string]` 表达.
`@Link` 中写 `Fn[[...] : Result[T, string]]`, 插件 Rust 函数写 `Result<T, String>`.
导出宏把 `Ok` 转为状态 0 与 `T`, 把 `Err` 转为状态 1 与错误信息;
宿主把二者分别包成 `Result/Ok[T]` 与 `Result/Err[string]`.

错误类型固定为 `string`, 参数与全局值不支持 `Result`, 也不支持嵌套 `Result`.
参数转换失败, 实参个数不符与 panic 都归为内部错误, 不进入 `Result`.

## 5. 字符串生命周期

宿主传入的字符串由宿主持有, 在调用期间有效.
插件需要保留时必须自行复制.

插件返回的字符串由插件写入线程暂存区, 在下一次插件入口开始时失效.
宿主在调用返回后立即复制, 因此调用方不需要释放返回的字符串.

## 6. 不透明句柄

不透明类型的类型表条目带一个可选的释放函数.

```c
typedef struct {
  KslStr name;
  void (*release)(uint64_t handle);
  uint32_t release_thread;
} KslOpaqueType;
```

句柄值是 `KslOpaqueValue`, 包含类型下标, 拥有标志与 `u64` 句柄.
拥有标志为 1 时, 宿主在最后一个引用消失时调用类型的释放函数;
为 0 时表示借用, 宿主不释放.

默认约定是句柄指向装箱值, 释放函数回收该装箱值.
需要自定义所有权时, 插件必须同时满足两点: 借用参数能解引用为有效对象;
拥有句柄的释放函数与创建方式匹配, 且只释放一次.

`release_thread` 取 `THREAD_ANY` 或 `THREAD_RUNTIME`.
`THREAD_RUNTIME` 表示释放入口必须在运行时线程执行; 其他线程触发的释放经队列交回调度线程.
`THREAD_ANY` 允许释放在句柄最后一个引用消失的线程执行, 插件必须保证线程安全.
动态库由宿主持有, 句柄与函数值都会保持动态库存活.

## 7. 生命周期入口

初始化与卸载入口都可选. 初始化不接受执行类别声明, 卸载通过线程亲和性字段声明执行位置.

```c
typedef uint32_t (*KslInitEntry)(KslValue *out);
typedef void (*KslUnloadEntry)(void);
typedef void (*KslEventPumpEntry)(void);
```

初始化入口在描述符校验通过后调用一次.
返回 `KSL_STATUS_OK` 表示成功; 其他状态按加载失败处理,
`out` 中的字符串作为错误信息, 此时不构造插件实例, 也不调用卸载入口.
初始化在模块加载时执行, 此时位于调度线程.
需要特定线程的宿主应在该线程驱动运行时; 运行时无法检测加载线程是否满足插件要求.

卸载入口在动态库卸载前调用一次, 不接受参数, 不返回状态.
`THREAD_ANY` 在最后一个插件引用消失的线程执行;
`THREAD_RUNTIME` 经释放队列交回调度线程执行, 与不透明句柄的释放共用同一队列.
队列中的全部释放任务执行完成后, 动态库才真正卸载.

### 7.1 事件泵

插件可以声明一个事件泵.
事件泵在调度线程上, 于执行片段之间与空闲时调用; CLI 中该线程就是进程主线程.
入口必须立即返回, 不得阻塞等待下一帧或下一事件.
事件泵 panic 由导出宏捕获并忽略, 插件其余入口不受影响.
交互会话不注册事件泵, 避免与终端输入争夺控制权.

## 8. 编写 Rust 插件

`ksl-plugin` crate 提供 ABI 定义, 值辅助与导出宏.
插件 crate 依赖它并声明 `crate-type = ["cdylib"]`.

函数默认以当前调用类别执行; `#[ksl_exec(io)]` 与 `#[ksl_exec(cpu)]`
分别声明 I/O 与 CPU 调用.
导出函数不接受 `main` 类别: 当前调用已经在调度线程执行.
不透明类型通过 `KslOpaque::RELEASE_THREAD` 声明释放线程.
初始化入口用 `#[ksl_init]` 标记, 卸载入口用 `#[ksl_unload]` 标记;
卸载入口用 `#[ksl_exec(main)]` 声明运行时线程亲和性; 初始化入口不接受类别声明.
事件泵用 `#[ksl_event_pump]` 标记, 在调度线程执行, 不接受参数与返回值.

```rust
use ksl_plugin::prelude::*;

pub struct Widget {
    value: i64,
}

impl KslOpaque for Widget {
    const NAME: &'static str = "Widget";
}

#[ksl_plugin(name = "demo", version = "0.1.0")]
mod plugin {
    use super::*;

    #[ksl_init]
    fn setup() -> Result<(), String> {
        Ok(())
    }

    #[ksl_unload]
    #[ksl_exec(main)]
    fn teardown() {}

    #[ksl_event_pump]
    fn pump() {}

    #[ksl_export]
    fn add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[ksl_export("Greet")]
    fn greet(name: &str) -> String {
        format!("hello {name}")
    }

    #[ksl_export]
    fn parse(value: &str) -> Result<i64, String> {
        value.parse().map_err(|_| "not an integer".to_string())
    }

    #[ksl_export("MakeWidget")]
    fn make_widget() -> Widget {
        Widget { value: 7 }
    }

    #[ksl_export]
    fn widget_value(widget: &Widget) -> i64 {
        widget.value
    }

    #[ksl_global("VERSION")]
    static VERSION_VALUE: i64 = 1;
}
```

`@Link` 使用 `name`, 即上例的 `MakeWidget` 与 `VERSION`, 而不是 Rust 源名
`make_widget` 与 `VERSION_VALUE`.

- `#[ksl_plugin]` 生成入口, 类型表, 函数表, 全局值表与生命周期包装.
- `#[ksl_export]` 标记函数; 可带字符串指定暴露给 ksl 的名字, 缺省用 Rust 函数名.
- `#[ksl_global]` 标记常量或静态值; 可带字符串指定暴露名.
- 不透明类型实现 `KslOpaque`, 提供 `NAME`, 默认以 `Box` 创建与释放句柄.
- 参数中的 `&T` 表示借用不透明句柄; 不透明返回值按值返回并创建新句柄.
- `&str` 与 `String` 映射到 ksl `string`; 标量按值传递.
- 返回 `Result<T, String>` 的函数声明为 `Result[T, string]`; `Err` 映射为 `Result/Err[string]`.
- `#[ksl_init]` 入口不接受参数, 返回 `()` 或 `Result<(), String>`; 后者的 `Err` 成为加载失败信息.
- `#[ksl_unload]` 入口不接受参数且返回 `()`; panic 被忽略.
- `#[ksl_event_pump]` 入口不接受参数且返回 `()`; 由调度循环调用, 不得阻塞.
- 宏为每个入口包一层 `catch_unwind`, panic 转成失败信息; 插件仍不应依赖跨 ABI 展开.

## 9. 加载与校验

检查阶段只验证 `@Link` 声明与签名范围, 不读取动态库.
声明中的签名超出支持范围时报 `PluginUnsupportedSignature`.

模块执行时按顺序完成:

1. 在模块目录的 `plugin/` 下按平台约定定位文件.
2. 加载动态库并调用 `ksl_plugin_init`.
3. 校验 ABI 版本与插件名.
4. 读取并去重类型表, 函数表与全局值表.
5. 调用初始化入口.
6. 按 `@Link` 声明校验类型名, 函数签名与全局值类型.
7. 注册事件泵.
8. 绑定函数值或读取全局值.

加载失败, 名字缺失, 签名不符, ABI 不符与调用失败都是宿主级运行时错误, 不返回 `Result`.

## 10. 限制

- 不描述结构体, 指针, 数组, 回调与变长参数; 这些由插件包装层内部处理.
- 一个动态库只允许一个 `ksl_plugin_init`, 一个 crate 只应有一个 `#[ksl_plugin]` 模块.
- 插件入口, 函数与释放函数不得跨 C ABI 展开 panic.
- 同一不透明类型在签名中应使用一致的路径写法, 避免生成重复类型条目.
- 可恢复错误的类型固定为 `string`; 更丰富的错误类型暂不进入 ABI.

---
updated: 2026-11-27
---

# ksl 原生插件

`@Link` 使用的动态库插件接口, 面向插件作者与运行时维护者.
语言层的 `@Link` 声明规则见 `docs/topics/language/modules.md` 第 2.1 节,
插件在宿主中的加载见 `docs/topics/implementation/architecture.md` 第 5 节.

## 1. 形态

插件是 Rust 动态库, 与宿主由同一工作区与同一工具链构建.
函数直接接收与返回 `ksl_value::value::Value`.
插件导出唯一入口 `ksl_native_plugin`, 返回静态描述符.
插件文件的目录, 文件名与路径规则见 `docs/topics/language/modules.md` 第 2.1 节;
链接阶段解析路径, 执行阶段加载动态库.

描述符中的名字是 UTF-8 字符串, 带长度, 不要求 NUL 结尾.
描述符, 类型表, 函数表与全局值表由插件静态持有, 在动态库卸载前保持有效.

## 2. 描述符

```rust
pub const NATIVE_ABI_VERSION: u32 = 1;

pub type NativeEntry = extern "Rust" fn(args: &[Value]) -> Result<Value, String>;
pub type NativeGlobalEntry = extern "Rust" fn() -> Value;
pub type NativeInitEntry = extern "Rust" fn() -> Result<(), String>;
pub type NativeUnloadEntry = extern "Rust" fn();
pub type NativeEventPumpEntry = extern "Rust" fn();

#[repr(C)]
pub struct NativePlugin {
    pub abi: u32,
    pub name: KslStr,
    pub version: KslStr,
    pub functions: *const NativeFunction,
    pub function_count: u32,
    pub types: *const NativeType,
    pub type_count: u32,
    pub globals: *const NativeGlobal,
    pub global_count: u32,
    pub init: Option<NativeInitEntry>,
    pub unload: Option<NativeUnloadEntry>,
    pub pump: Option<NativeEventPumpEntry>,
}
```

`abi` 必须等于 `NATIVE_ABI_VERSION`, `name` 必须等于 `@Link` 中的插件名.
计数为 0 时对应指针可以为空; 计数非 0 时指针必须非空.
初始化与卸载入口可选.

## 3. 函数

宿主按 `@Link` 声明传入参数. 插件函数自行校验实参类型;
`Err` 表示插件内部失败.

- 声明返回 `Result[T, string]` 时, `Err` 映射为 `Result/Err[string]`.
- 其他声明下, `Err` 报运行时错误.
- 入口 panic 由导出宏捕获, 转为 `Err`.

参数与返回值都是 `Value`, `@Link` 中的类型不受限制.
宏只解码下列 Rust 签名; 其他类型写成 `Value` 或 `&Value`, 由插件读写.

执行类别:

| 类别                | 执行位置   | 契约                         |
| ------------------- | ---------- | ---------------------------- |
| `EXECUTION_CURRENT` | 调度线程   | 入口很快返回, 不等待外部事件 |
| `EXECUTION_IO`      | I/O 线程池 | 入口阻塞在外部事件上         |
| `EXECUTION_CPU`     | CPU 线程池 | 入口做长计算                 |

I/O 与 CPU 调用期间 Actor 挂起, 完成后经 I/O 完成事件恢复.
实参会跨线程传递, 插件必须保证入口线程安全.

类型化导出由宏生成参数解码与返回值编码:

```rust
#[ksl_native_export]
fn greet(name: &str) -> String {
    format!("hello {name}")
}
```

支持的类型:

- 参数: 标量, `&str`, `String`, `&T` 不透明类型, `Value`, `&Value`;
- 返回: 标量, `&str`, `String`, `T` 不透明类型, `()`, `Value`, `Result<T, String>`.

单参数 `&[Value]` 视为原始入口, 宏不做解码:

```rust
#[ksl_native_export]
fn sum(args: &[Value]) -> Result<Value, String> {
    let [value] = args else {
        return Err("参数应为列表".to_string());
    };
    let len = value.list_len().ok_or_else(|| "参数应为列表".to_string())?;
    let mut total = 0;
    for index in 0..len {
        total += value
            .list_get(index)
            .and_then(|item| item.as_i64())
            .ok_or_else(|| "元素应为 i64".to_string())?;
    }
    Ok(Value::I64(total))
}
```

## 4. 不透明类型

插件资源以引用计数共享, 由插件实现 `NativeOpaque`:

```rust
pub trait NativeOpaque: Any + Send + Sync {
    fn type_name(&self) -> &'static str;
}
```

资源名在 `#[ksl_native_plugin(types = ["Widget", ...])]` 中列出,
与 `@Link` 的类型声明对应.
插件用 `Value::native_opaque(value)` 包装资源, 用 `Value::as_opaque::<T>()` 取回借用.
最后一个引用消失时执行析构; 析构线程由持有者决定, 需要调度线程的资源应自行处理.

## 5. 全局值

```rust
#[ksl_native_global]
const VERSION: i64 = 1;

#[ksl_native_global]
static TITLE: &str = "demo";
```

支持标量与 `&str`. 全局值在模块执行时读取一次并绑定为 ksl 值.

## 6. 生命周期

```rust
#[ksl_native_init]
fn setup() -> Result<(), String> { Ok(()) }

#[ksl_native_unload]
fn teardown() {}

#[ksl_native_event_pump]
fn pump() {}
```

- 初始化在描述符校验后调用一次; 返回 `Err` 时加载失败.
- 卸载在插件实例释放时调用一次; 动态库保持映射到宿主结束, 卸载钩子不表示代码被卸载.
- 插件资源值不应长于宿主; 宿主结束时动态库才释放.
- 事件泵在调度线程上, 于执行片段之间与空闲时调用, 必须立即返回.
- 交互会话不注册事件泵.

## 7. 示例

```rust
use ksl_value::ksl_native_plugin;
use ksl_value::value::{NativeOpaque, Value};

pub struct Widget {
    value: i64,
}

impl NativeOpaque for Widget {
    fn type_name(&self) -> &'static str {
        "Widget"
    }
}

#[ksl_native_plugin(name = "demo", types = ["Widget"])]
mod plugin {
    use super::*;

    #[ksl_native_global]
    const VERSION: i64 = 1;

    #[ksl_native_export]
    fn add(left: i64, right: i64) -> i64 {
        left + right
    }

    #[ksl_native_export("MakeWidget")]
    fn make_widget() -> Widget {
        Widget { value: 7 }
    }

    #[ksl_native_export]
    fn widget_value(widget: &Widget) -> i64 {
        widget.value
    }
}
```

对应的 `@Link`:

```text
@Link["demo", { ^Widget }, {
  add : Fn[[i64, i64] : i64],
  MakeWidget : Fn[[] : Widget],
  widget_value : Fn[[Widget] : i64],
  VERSION : i64
}];
```

## 8. 加载与校验

1. 在模块目录的 `plugin/` 下按平台约定定位文件.
2. 加载动态库并调用 `ksl_native_plugin`.
3. 校验 ABI 版本, 插件名与描述符指针.
4. 读取类型, 函数与全局值; 重名报错.
5. 调用初始化入口; 注册事件泵.

`@Link` 是唯一的签名来源, 描述符不重复声明参数与返回类型.
宿主只在声明返回 `Result[T, string]` 时识别可恢复失败, 其他类型按原样传递.
`@Link` 中声明的类型或函数缺失时报运行时错误.

## 9. 约束

- 插件与宿主必须使用同一工具链; 不同版本不保证类型布局.
- 进程使用系统分配器; 宿主与插件各持运行时副本, 资源必须能跨副本释放.
- 入口不得展开 panic; 宏统一捕获.
- 值不得跨调用保存; 需要长期持有的资源用 `NativeOpaque`.
- 插件线程不能直接执行 ksl 代码.

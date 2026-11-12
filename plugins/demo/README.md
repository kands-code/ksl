# ksl demo 测试插件

验证原生插件接口的独立插件, 供 `ksl-runtime` 集成测试使用, 插件只依赖 `ksl-value`. 覆盖:

- 标量, f32, i32, 字符串, 字符, 列表与 `Result`
- 不透明句柄 `Widget` 与跨模块使用
- 全局值, 初始化入口, 卸载入口与事件泵
- `current`, `io` 与 `cpu` 执行类别

`runtime.ksl` 由 `ksl-runtime` 的集成测试构建插件后执行;
`@Main` 的状态参数是卸载标记文件路径.

手动构建, 在 `plugins/demo` 下执行:

```sh
cargo build -p ksl-demo-plugin
mkdir -p plugin
cp ../../target/debug/libksl_demo_plugin.<扩展名> plugin/libdemo.<扩展名>
```

macOS 的扩展名是 `dylib`, Linux 是 `so`, Windows 是 `dll`.
插件产物目录 `plugin/` 被 `.gitignore` 忽略.

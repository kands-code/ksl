# ksl raylib 插件

raylib 的 ksl 包装插件, 同时用作原生插件示例.

插件只暴露标量, 字符串, `nil`, `Result` 与不透明类型; 结构体, 指针与数组留在 Rust 包装层.
类型与 raylib 一致: C `int` 对应 `i32`, `float` 对应 `f32`.
`unsigned int` 参数用 `u32`: `SetConfigFlags` 的标志,
`SaveStorageValue` 与 `LoadStorageValue` 的位置, 以及 `ColorToInt` 的打包值.

值类型都是不透明类型, 由构造函数创建, 由访问器读取:

- `Color`: `Rgba` 与 `GetColor` 构造, `Fade`, `ColorTint` 等返回新值.
- `Vector2`: `Vec2[x, y]` 构造, `Vec2X` 与 `Vec2Y` 读取; 矢量运算见 `Vec2Add` 等辅助函数.
- `Rectangle`: `Rect[x, y, width, height]` 构造, `RectX`, `RectWidth`, `RectPosition` 等读取.
- `Texture` 与 `Image`: 由加载或生成函数返回; `Image` 驻留 CPU 内存, `Texture` 驻留显存.
- `Font`, `RenderTexture` 与 `Codepoints`: 由加载函数返回, 由显式卸载或析构释放.
- `AudioStream`: 由 `LoadAudioStream` 创建, 由 `UnloadAudioStream` 或析构释放.
- `Shader`: 由 `LoadShaderFromFragment` 创建, 由 `UnloadShader` 或析构释放.

## 1. 目录

| 路径               | 内容                       |
| ------------------ | -------------------------- |
| `src/lib.rs`       | 导出函数与不透明类型       |
| `src/ffi.rs`       | raylib C 接口与值转换      |
| `src/resources.rs` | 不透明资源, 拥有句柄与释放 |
| `Cargo.toml`       | workspace 成员             |
| `build.rs`         | 按 `RAYLIB_DIR` 链接       |
| `build.sh`         | 构建并复制插件到 `plugin/` |
| `raylib.ksl`       | `@Link` 声明, 常量与辅助   |
| `lib/`             | 示例共享的 ksl 模块        |
| `*.ksl`            | 示例                       |

示例与 `raylib.ksl` 平铺在插件目录, 共享模块放在 `lib/`.
模块根是主文件所在目录, 子目录中的模块不能导入上级模块.

## 2. 构建

插件是 workspace 成员. `default-members` 让默认构建跳过插件;
`--workspace` 会检查真实插件, 因此需要 raylib.

raylib 发布包放在 `vendor/` 下, 目录名沿用发布包, 例如 `raylib-6.2_macos`;
`build.rs` 按目标平台选择匹配目录, 也可以用 `RAYLIB_DIR` 直接指定.
macOS 优先静态链接 `libraylib.a` 并链接系统框架, 生成的插件不依赖 raylib 动态库;
其他平台回退到动态链接, 运行时需要能定位 raylib 动态库.

```sh
# 已把 raylib-6.2_macos 放到 vendor/
./build.sh

# 或直接指定目录
RAYLIB_DIR=/private/tmp/raylib-6.2_macos ./build.sh
```

构建产物为 `plugin/libraylib.dylib` 或 `plugin/libraylib.so`, 该目录被 `.gitignore` 忽略.
`@Link` 中的插件名是 `raylib`, 因此文件名必须是平台对应的 `libraylib` 动态库名.

## 3. 运行

```sh
# 交互运行; 关闭窗口结束
cargo run -p ksl-cli -- run plugins/raylib/core_storage_values.ksl
cargo run -p ksl-cli -- run plugins/raylib/core_window_flags.ksl
cargo run -p ksl-cli -- run plugins/raylib/shapes_basics.ksl
cargo run -p ksl-cli -- run plugins/raylib/shapes_double_pendulum.ksl
cargo run -p ksl-cli -- run plugins/raylib/textures_basics.ksl
cargo run -p ksl-cli -- run plugins/raylib/shaders_game_of_life.ksl
cargo run -p ksl-cli -- run plugins/raylib/audio_raw_stream.ksl
# 频谱示例的 FFT 较重, 建议使用 release 构建.
cargo run --release -p ksl-cli -- run plugins/raylib/audio_spectrum.ksl
```

存储接口读写当前工作目录的 `storage.data`.
`core_storage_values.ksl` 先切换到模块目录下的 `tmp/`, 存储文件写在该目录.

## 4. 示例

| 上游示例                    | 文件                         | 说明                         |
| --------------------------- | ---------------------------- | ---------------------------- |
| `core_storage_values`       | `core_storage_values.ksl`    | 存储读写与按键交互           |
| `core_window_flags`         | `core_window_flags.ksl`      | 窗口状态, 显示器与鼠标信息   |
| `shapes_double_pendulum`    | `shapes_double_pendulum.ksl` | 居中固定点与完整摆动轨迹     |
| 无                          | `shapes_basics.ksl`          | 2D 形状, 颜色与碰撞总览      |
| 无                          | `textures_basics.ksl`        | 图像生成, 纹理创建与绘制     |
| `shaders_game_of_life`      | `shaders_game_of_life.ksl`   | 片段着色器与渲染纹理交替     |
| `audio_raw_stream`          | `audio_raw_stream.ksl`       | 原始音频流, 正弦波与声像     |
| `audio_spectrum_visualizer` | `audio_spectrum.ksl`         | 频谱水瀑布, 对数频段与着色器 |

与上游示例的差异:

- 双摆使用 800x640 窗口并居中固定点, 两节摆杆的完整可达范围都在窗口内;
  不启用 `FLAG_WINDOW_HIGHDPI`.
- `core_window_flags.ksl` 可切换全屏, 最大化, 置顶与光标显隐.
- `shapes_basics.ksl` 启动时校验值与碰撞的纯函数结果, 之后绘制形状总览.
- `textures_basics.ksl` 用 `GenImageChecked` 现场生成棋盘, 不需要图像素材;
  启动时校验图像与纹理数据.
- `shaders_game_of_life.ksl` 用键盘控制, 随机方块初始化, 左侧栏显示 FPS 与提示,
  不使用 raygui 与预设图片.
- `audio_raw_stream.ksl` 的正弦样本由 ksl 生成, 界面只显示频率与声像.
- `audio_spectrum.ksl` 加载 `resources/audio/country.mp3`, 单声道混合后按块推送到音频流;
  对最近 512 个样本做 ksl 侧 FFT, 按对数频段合并为 64 个值, 覆盖约 16.5 kHz,
  写入环形纹理的下一行;
  `resources/shaders/fft.fs` 按时间滚动, 用彩虹色和亮度显示能量.
  512 点 FFT 在 release 构建下保持 60 FPS, 调试构建明显更慢.

以下示例支持按 S 把当前帧写到模块目录下的 `tmp/`:

| 示例                | 文件                        |
| ------------------- | --------------------------- |
| `core_window_flags` | `tmp/core_window_flags.png` |
| `shapes_basics`     | `tmp/shapes_basics.png`     |
| `textures_basics`   | `tmp/textures_basics.png`   |

## 5. 覆盖范围

- 窗口: 初始化, 关闭, 状态查询与切换, 标题, 图标, 尺寸, 位置, 透明度, 全屏,
  最大化与还原, 事件等待, 帧时间, FPS, 时间, 配置标志, 日志等级, 截图.
- 显示器: 数量, 当前显示器, 名称, 尺寸, 物理尺寸, 刷新率与位置.
- 输入: 键盘按下/重复/按住/释放, 字符输入, 键码与键名,
  鼠标位置/位移/滚轮/按键, 光标显隐与形状, 随机数.
- 值类型: `Vector2` 与 `Rectangle` 的构造与访问, `Vec2Add` 等矢量辅助函数.
- 绘制: 像素, 线, 贝塞尔线, 虚线, 三角形, 矩形, 渐变矩形, 圆角矩形,
  旋转矩形, 多边形, 圆, 渐变圆, 扇形, 椭圆, 圆环, 文字.
- 碰撞: 矩形, 圆, 圆与矩形, 圆与线, 点与矩形/圆/三角形/线, 以及碰撞矩形.
- 颜色: 构造, 打包值, 相等比较, 透明度, 亮度, 对比度, 色相, 混合与插值.
- 字体: 默认字体, 加载与卸载, 按文本码点加载, 图集尺寸与绘制, 过滤, 行距,
  文本测量与旋转文本.
- 码点: 加载, 计数, 逐项读取, 卸载.
- 纹理: 从文件或图像加载, 卸载, 有效性, 尺寸, 更新, 过滤, 包裹, mipmap,
  整纹理, 位置, 旋转缩放, 源矩形与目标矩形绘制.
- 图像: 从文件或纹理加载, 卸载, 有效性, 尺寸, 纯色与棋盘生成, 像素读写.
- 渲染纹理: 加载与卸载, 进入与退出, 过滤, 尺寸, 整纹理绘制.
- 系统: 剪贴板读写, 工作目录切换, 存储读写.
- 着色器: 片段着色器加载, float, vec2 与纹理 uniform, shader 模式.
- 音频: 设备初始化与关闭, 波形加载, 单声道分块读取, 原始音频流,
  缓冲推送, 播放与声像/音量/音高; 频谱分析由 ksl 侧模块完成.
- 常量: 颜色, 键码, 鼠标按钮与光标, 窗口标志, 日志等级, 纹理过滤与包裹.

## 6. 接口限制

- 回调. 原生插件暂不支持回调; raylib 的事件回调留在包装层.
  音频流用 `IsAudioStreamProcessed` 轮询与 `UpdateAudioStream` 推送, 不经过回调.
- 数组与切片. 包装层不暴露数组, `DrawLineStrip`, `DrawTriangleFan`,
  `DrawTriangleStrip` 与 `DrawSpline*` 未绑定.
- 顶点顺序. 填充三角形要求屏幕坐标下逆时针, 顺序相反会被背面剔除.
- 子对象. 不透明类型返回值总是拥有句柄, 例如 `Font.texture` 需要额外访问器, 不能借用返回.
- 可变引用. 原生接口不支持 `&mut` 不透明参数, 资源内部可变状态由包装层的 `Mutex` 保护.
- 数值. 原生接口支持 f32 与 i32, raylib 的 `float` 与 C `int` 直接对应, 不做隐式转换.
- 角度. 扇形, 圆环与多边形等角度参数的单位是度, 与 raylib 一致.
- 文本. 默认字体只支持 ASCII; 示例的界面文本只用 ASCII.
- 着色器. 只暴露片段着色器, float, vec2 与纹理 uniform; 需要 OpenGL 3.3 或 ES2.
- 签名来源. `@Link` 的签名手写维护; 描述符只带名字, 不携带类型.
  声明与导出不一致时, 模块执行阶段按缺失导出报错.

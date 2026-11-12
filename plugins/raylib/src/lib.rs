//! ksl 的 raylib 包装插件.
//!
//! 只暴露标量, 字符串与不透明类型; 结构体与 C 指针留在本层内部.
//! 值类型由构造函数创建, 由访问器读取; 资源句柄由加载函数返回, 由显式卸载或析构释放.

mod ffi;
mod resources;

use std::ffi::{CStr, c_int, c_void};

use ksl_value::ksl_native_plugin;
use ksl_value::value::Value;

// 绑定名与 link_name 一一对应.
#[allow(clippy::wildcard_imports)]
use self::ffi::*;
use self::resources::{
    AudioStreamRes, CodepointsRes, ColorRes, FontRes, Handle, ImageRes, RectangleRes,
    RenderTextureRes, ShaderRes, TextureRes, Vector2Res, WaveRes, set_audio_open,
    set_window_open, unload_font_safely, unload_render_texture_safely, unload_texture_safely,
};

fn storage_file() -> std::path::PathBuf {
    std::path::PathBuf::from("storage.data")
}

#[ksl_native_plugin(
    name = "raylib",
    version = "0.1.0",
    types = [
        "Color",
        "Font",
        "RenderTexture",
        "Codepoints",
        "Vector2",
        "Rectangle",
        "Texture",
        "Image",
        "AudioStream",
        "Wave",
        "Shader"
    ]
)]
mod plugin {
    use super::*;

    // 值类型
    #[ksl_native_export("Vec2")]
    fn vec2(x: f32, y: f32) -> Vector2Res {
        Vector2Res {
            inner: vector(x, y),
        }
    }

    #[ksl_native_export("Vec2X")]
    fn vec2_x(value: &Vector2Res) -> f32 {
        value.inner.x
    }

    #[ksl_native_export("Vec2Y")]
    fn vec2_y(value: &Vector2Res) -> f32 {
        value.inner.y
    }

    #[ksl_native_export("Rect")]
    fn rect(x: f32, y: f32, width: f32, height: f32) -> RectangleRes {
        RectangleRes {
            inner: rectangle(x, y, width, height),
        }
    }

    #[ksl_native_export("RectX")]
    fn rect_x(value: &RectangleRes) -> f32 {
        value.inner.x
    }

    #[ksl_native_export("RectY")]
    fn rect_y(value: &RectangleRes) -> f32 {
        value.inner.y
    }

    #[ksl_native_export("RectWidth")]
    fn rect_width(value: &RectangleRes) -> f32 {
        value.inner.width
    }

    #[ksl_native_export("RectHeight")]
    fn rect_height(value: &RectangleRes) -> f32 {
        value.inner.height
    }

    #[ksl_native_export("RectPosition")]
    fn rect_position(value: &RectangleRes) -> Vector2Res {
        Vector2Res {
            inner: vector(value.inner.x, value.inner.y),
        }
    }

    #[ksl_native_export("RectSize")]
    fn rect_size(value: &RectangleRes) -> Vector2Res {
        Vector2Res {
            inner: vector(value.inner.width, value.inner.height),
        }
    }

    // 窗口与系统
    #[ksl_native_export("InitWindow")]
    fn init_window(width: i32, height: i32, title: &str) {
        let Some(title) = cstring(title) else {
            return;
        };
        unsafe { rl_init_window(width, height, title.as_ptr()) };
        set_window_open(true);
    }

    #[ksl_native_export("CloseWindow")]
    fn close_window() {
        unsafe { rl_close_window() };
        set_window_open(false);
    }

    #[ksl_native_export("WindowShouldClose")]
    fn window_should_close() -> bool {
        unsafe { rl_window_should_close() }
    }

    #[ksl_native_export("IsWindowReady")]
    fn is_window_ready() -> bool {
        unsafe { rl_is_window_ready() }
    }

    #[ksl_native_export("IsWindowFullscreen")]
    fn is_window_fullscreen() -> bool {
        unsafe { rl_is_window_fullscreen() }
    }

    #[ksl_native_export("IsWindowHidden")]
    fn is_window_hidden() -> bool {
        unsafe { rl_is_window_hidden() }
    }

    #[ksl_native_export("IsWindowMinimized")]
    fn is_window_minimized() -> bool {
        unsafe { rl_is_window_minimized() }
    }

    #[ksl_native_export("IsWindowMaximized")]
    fn is_window_maximized() -> bool {
        unsafe { rl_is_window_maximized() }
    }

    #[ksl_native_export("IsWindowFocused")]
    fn is_window_focused() -> bool {
        unsafe { rl_is_window_focused() }
    }

    #[ksl_native_export("IsWindowResized")]
    fn is_window_resized() -> bool {
        unsafe { rl_is_window_resized() }
    }

    #[ksl_native_export("IsWindowState")]
    fn is_window_state(flag: u32) -> bool {
        unsafe { rl_is_window_state(flag) }
    }

    #[ksl_native_export("SetWindowState")]
    fn set_window_state(flags: u32) {
        unsafe { rl_set_window_state(flags) };
    }

    #[ksl_native_export("ClearWindowState")]
    fn clear_window_state(flags: u32) {
        unsafe { rl_clear_window_state(flags) };
    }

    #[ksl_native_export("ToggleFullscreen")]
    fn toggle_fullscreen() {
        unsafe { rl_toggle_fullscreen() };
    }

    #[ksl_native_export("MaximizeWindow")]
    fn maximize_window() {
        unsafe { rl_maximize_window() };
    }

    #[ksl_native_export("MinimizeWindow")]
    fn minimize_window() {
        unsafe { rl_minimize_window() };
    }

    #[ksl_native_export("RestoreWindow")]
    fn restore_window() {
        unsafe { rl_restore_window() };
    }

    #[ksl_native_export("SetWindowIcon")]
    fn set_window_icon(image: &ImageRes) {
        if let Some(inner) = image.inner.get() {
            unsafe { rl_set_window_icon(inner) };
        }
    }

    #[ksl_native_export("SetWindowTitle")]
    fn set_window_title(title: &str) {
        let Some(title) = cstring(title) else {
            return;
        };
        unsafe { rl_set_window_title(title.as_ptr()) };
    }

    #[ksl_native_export("SetWindowPosition")]
    fn set_window_position(x: i32, y: i32) {
        unsafe { rl_set_window_position(x, y) };
    }

    #[ksl_native_export("SetWindowSize")]
    fn set_window_size(width: i32, height: i32) {
        unsafe { rl_set_window_size(width, height) };
    }

    #[ksl_native_export("SetWindowMinSize")]
    fn set_window_min_size(width: i32, height: i32) {
        unsafe { rl_set_window_min_size(width, height) };
    }

    #[ksl_native_export("SetWindowMaxSize")]
    fn set_window_max_size(width: i32, height: i32) {
        unsafe { rl_set_window_max_size(width, height) };
    }

    /// 窗口不透明度, 取值 [0, 1].
    #[ksl_native_export("SetWindowOpacity")]
    fn set_window_opacity(opacity: f32) {
        unsafe { rl_set_window_opacity(opacity) };
    }

    #[ksl_native_export("SetWindowFocused")]
    fn set_window_focused() {
        unsafe { rl_set_window_focused() };
    }

    #[ksl_native_export("GetRenderWidth")]
    fn get_render_width() -> i32 {
        unsafe { rl_get_render_width() }
    }

    #[ksl_native_export("GetRenderHeight")]
    fn get_render_height() -> i32 {
        unsafe { rl_get_render_height() }
    }

    #[ksl_native_export("GetWindowPosition")]
    fn get_window_position() -> Vector2Res {
        Vector2Res {
            inner: unsafe { rl_get_window_position() },
        }
    }

    #[ksl_native_export("GetWindowScaleDPI")]
    fn get_window_scale_dpi() -> Vector2Res {
        Vector2Res {
            inner: unsafe { rl_get_window_scale_dpi() },
        }
    }

    #[ksl_native_export("EnableEventWaiting")]
    fn enable_event_waiting() {
        unsafe { rl_enable_event_waiting() };
    }

    #[ksl_native_export("DisableEventWaiting")]
    fn disable_event_waiting() {
        unsafe { rl_disable_event_waiting() };
    }

    #[ksl_native_export("SetExitKey")]
    fn set_exit_key(key: i32) {
        unsafe { rl_set_exit_key(key) };
    }

    #[ksl_native_export("SetConfigFlags")]
    fn set_config_flags(flags: u32) {
        unsafe { rl_set_config_flags(flags) };
    }

    /// 日志等级, 取值见 `LOG_*` 常量.
    #[ksl_native_export("SetTraceLogLevel")]
    fn set_trace_log_level(level: i32) {
        unsafe { rl_set_trace_log_level(level) };
    }

    #[ksl_native_export("SetTargetFPS")]
    fn set_target_fps(fps: i32) {
        unsafe { rl_set_target_fps(fps) };
    }

    #[ksl_native_export("GetFrameTime")]
    fn get_frame_time() -> f32 {
        unsafe { rl_get_frame_time() }
    }

    #[ksl_native_export("GetFPS")]
    fn get_fps() -> i32 {
        unsafe { rl_get_fps() }
    }

    #[ksl_native_export("GetScreenWidth")]
    fn get_screen_width() -> i32 {
        unsafe { rl_get_screen_width() }
    }

    #[ksl_native_export("GetScreenHeight")]
    fn get_screen_height() -> i32 {
        unsafe { rl_get_screen_height() }
    }

    #[ksl_native_export("DrawFPS")]
    fn draw_fps(x: i32, y: i32) {
        unsafe { rl_draw_fps(x, y) };
    }

    // 显示器
    #[ksl_native_export("GetMonitorCount")]
    fn get_monitor_count() -> i32 {
        unsafe { rl_get_monitor_count() }
    }

    #[ksl_native_export("GetCurrentMonitor")]
    fn get_current_monitor() -> i32 {
        unsafe { rl_get_current_monitor() }
    }

    #[ksl_native_export("GetMonitorPosition")]
    fn get_monitor_position(monitor: i32) -> Vector2Res {
        Vector2Res {
            inner: unsafe { rl_get_monitor_position(monitor) },
        }
    }

    #[ksl_native_export("GetMonitorWidth")]
    fn get_monitor_width(monitor: i32) -> i32 {
        unsafe { rl_get_monitor_width(monitor) }
    }

    #[ksl_native_export("GetMonitorHeight")]
    fn get_monitor_height(monitor: i32) -> i32 {
        unsafe { rl_get_monitor_height(monitor) }
    }

    #[ksl_native_export("GetMonitorPhysicalWidth")]
    fn get_monitor_physical_width(monitor: i32) -> i32 {
        unsafe { rl_get_monitor_physical_width(monitor) }
    }

    #[ksl_native_export("GetMonitorPhysicalHeight")]
    fn get_monitor_physical_height(monitor: i32) -> i32 {
        unsafe { rl_get_monitor_physical_height(monitor) }
    }

    #[ksl_native_export("GetMonitorRefreshRate")]
    fn get_monitor_refresh_rate(monitor: i32) -> i32 {
        unsafe { rl_get_monitor_refresh_rate(monitor) }
    }

    #[ksl_native_export("GetMonitorName")]
    fn get_monitor_name(monitor: i32) -> String {
        let pointer = unsafe { rl_get_monitor_name(monitor) };
        if pointer.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }

    /// 启动以来的秒数.
    #[ksl_native_export("GetTime")]
    fn get_time() -> f64 {
        unsafe { rl_get_time() }
    }

    // 输入
    #[ksl_native_export("IsKeyPressed")]
    fn is_key_pressed(key: i32) -> bool {
        unsafe { rl_is_key_pressed(key) }
    }

    #[ksl_native_export("IsKeyDown")]
    fn is_key_down(key: i32) -> bool {
        unsafe { rl_is_key_down(key) }
    }

    #[ksl_native_export("IsKeyReleased")]
    fn is_key_released(key: i32) -> bool {
        unsafe { rl_is_key_released(key) }
    }

    #[ksl_native_export("IsKeyUp")]
    fn is_key_up(key: i32) -> bool {
        unsafe { rl_is_key_up(key) }
    }

    #[ksl_native_export("IsKeyPressedRepeat")]
    fn is_key_pressed_repeat(key: i32) -> bool {
        unsafe { rl_is_key_pressed_repeat(key) }
    }

    /// 读取一个已按键码; 没有输入时返回 0.
    #[ksl_native_export("GetKeyPressed")]
    fn get_key_pressed() -> i32 {
        unsafe { rl_get_key_pressed() }
    }

    /// 按键在当前布局下的名字; 无法识别时返回空字符串.
    #[ksl_native_export("GetKeyName")]
    fn get_key_name(key: i32) -> String {
        let pointer = unsafe { rl_get_key_name(key) };
        if pointer.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }

    /// 读取一个已按下字符; 没有输入时返回 0.
    #[ksl_native_export("GetCharPressed")]
    fn get_char_pressed() -> i32 {
        unsafe { rl_get_char_pressed() }
    }

    #[ksl_native_export("IsMouseButtonPressed")]
    fn is_mouse_button_pressed(button: i32) -> bool {
        unsafe { rl_is_mouse_button_pressed(button) }
    }

    #[ksl_native_export("IsMouseButtonDown")]
    fn is_mouse_button_down(button: i32) -> bool {
        unsafe { rl_is_mouse_button_down(button) }
    }

    #[ksl_native_export("IsMouseButtonReleased")]
    fn is_mouse_button_released(button: i32) -> bool {
        unsafe { rl_is_mouse_button_released(button) }
    }

    #[ksl_native_export("IsMouseButtonUp")]
    fn is_mouse_button_up(button: i32) -> bool {
        unsafe { rl_is_mouse_button_up(button) }
    }

    #[ksl_native_export("GetMouseX")]
    fn get_mouse_x() -> i32 {
        unsafe { rl_get_mouse_x() }
    }

    #[ksl_native_export("GetMouseY")]
    fn get_mouse_y() -> i32 {
        unsafe { rl_get_mouse_y() }
    }

    #[ksl_native_export("GetMousePosition")]
    fn get_mouse_position() -> Vector2Res {
        Vector2Res {
            inner: unsafe { rl_get_mouse_position() },
        }
    }

    /// 上一帧到当前帧的鼠标位移.
    #[ksl_native_export("GetMouseDelta")]
    fn get_mouse_delta() -> Vector2Res {
        Vector2Res {
            inner: unsafe { rl_get_mouse_delta() },
        }
    }

    #[ksl_native_export("SetMousePosition")]
    fn set_mouse_position(x: i32, y: i32) {
        unsafe { rl_set_mouse_position(x, y) };
    }

    #[ksl_native_export("SetMouseOffset")]
    fn set_mouse_offset(offset_x: i32, offset_y: i32) {
        unsafe { rl_set_mouse_offset(offset_x, offset_y) };
    }

    #[ksl_native_export("SetMouseScale")]
    fn set_mouse_scale(scale_x: f32, scale_y: f32) {
        unsafe { rl_set_mouse_scale(scale_x, scale_y) };
    }

    #[ksl_native_export("GetMouseWheelMove")]
    fn get_mouse_wheel_move() -> f32 {
        unsafe { rl_get_mouse_wheel_move() }
    }

    #[ksl_native_export("GetMouseWheelMoveV")]
    fn get_mouse_wheel_move_v() -> Vector2Res {
        Vector2Res {
            inner: unsafe { rl_get_mouse_wheel_move_v() },
        }
    }

    #[ksl_native_export("SetMouseCursor")]
    fn set_mouse_cursor(cursor: i32) {
        unsafe { rl_set_mouse_cursor(cursor) };
    }

    // 光标
    #[ksl_native_export("ShowCursor")]
    fn show_cursor() {
        unsafe { rl_show_cursor() };
    }

    #[ksl_native_export("HideCursor")]
    fn hide_cursor() {
        unsafe { rl_hide_cursor() };
    }

    #[ksl_native_export("IsCursorHidden")]
    fn is_cursor_hidden() -> bool {
        unsafe { rl_is_cursor_hidden() }
    }

    #[ksl_native_export("EnableCursor")]
    fn enable_cursor() {
        unsafe { rl_enable_cursor() };
    }

    #[ksl_native_export("DisableCursor")]
    fn disable_cursor() {
        unsafe { rl_disable_cursor() };
    }

    #[ksl_native_export("GetRandomValue")]
    fn get_random_value(min: i32, max: i32) -> i32 {
        unsafe { rl_get_random_value(min, max) }
    }

    #[ksl_native_export("SetRandomSeed")]
    fn set_random_seed(seed: u32) {
        unsafe { rl_set_random_seed(seed) };
    }

    // 剪贴板与存储
    #[ksl_native_export("SetClipboardText")]
    fn set_clipboard_text(text: &str) {
        let Some(text) = cstring(text) else {
            return;
        };
        unsafe { rl_set_clipboard_text(text.as_ptr()) };
    }

    #[ksl_native_export("GetClipboardText")]
    fn get_clipboard_text() -> String {
        let pointer = unsafe { rl_get_clipboard_text() };
        if pointer.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    }

    /// 把当前帧缓冲写成图片文件, 格式由扩展名决定.
    ///
    /// 在 `EndDrawing` 之后调用; 绘制批次在 `EndDrawing` 中提交.
    #[ksl_native_export("TakeScreenshot")]
    fn take_screenshot(path: &str) {
        let Some(path) = cstring(path) else {
            return;
        };
        unsafe { rl_take_screenshot(path.as_ptr()) };
    }

    #[ksl_native_export("ChangeDirectory")]
    fn change_directory(path: &str) -> bool {
        let Some(path) = cstring(path) else {
            return false;
        };
        unsafe { rl_change_directory(path.as_ptr()) == 0 }
    }

    #[ksl_native_export("SaveStorageValue")]
    fn save_storage_value(position: u32, value: i32) -> bool {
        let Ok(position) = usize::try_from(position) else {
            return false;
        };
        let Some(start) = position.checked_mul(4) else {
            return false;
        };
        let Some(end) = start.checked_add(4) else {
            return false;
        };
        let path = storage_file();
        let mut data = std::fs::read(&path).unwrap_or_default();
        if data.len() < end {
            data.resize(end, 0);
        }
        data[start..end].copy_from_slice(&value.to_ne_bytes());
        std::fs::write(&path, data).is_ok()
    }

    #[ksl_native_export("LoadStorageValue")]
    fn load_storage_value(position: u32) -> i32 {
        let Ok(position) = usize::try_from(position) else {
            return 0;
        };
        let Some(start) = position.checked_mul(4) else {
            return 0;
        };
        let Some(end) = start.checked_add(4) else {
            return 0;
        };
        let data = std::fs::read(storage_file()).unwrap_or_default();
        if data.len() < end {
            return 0;
        }
        i32::from_ne_bytes([data[start], data[start + 1], data[start + 2], data[start + 3]])
    }

    // 绘制
    #[ksl_native_export("BeginDrawing")]
    fn begin_drawing() {
        unsafe { rl_begin_drawing() };
    }

    #[ksl_native_export("EndDrawing")]
    fn end_drawing() {
        unsafe { rl_end_drawing() };
    }

    #[ksl_native_export("ClearBackground")]
    fn clear_background(color: &ColorRes) {
        unsafe { rl_clear_background(color.inner) };
    }

    #[ksl_native_export("DrawPixel")]
    fn draw_pixel(x: i32, y: i32, color: &ColorRes) {
        unsafe { rl_draw_pixel(x, y, color.inner) };
    }

    #[ksl_native_export("DrawPixelV")]
    fn draw_pixel_v(position: &Vector2Res, color: &ColorRes) {
        unsafe { rl_draw_pixel_v(position.inner, color.inner) };
    }

    #[ksl_native_export("DrawLine")]
    fn draw_line(start_x: i32, start_y: i32, end_x: i32, end_y: i32, color: &ColorRes) {
        unsafe { rl_draw_line(start_x, start_y, end_x, end_y, color.inner) };
    }

    #[ksl_native_export("DrawLineV")]
    fn draw_line_v(start: &Vector2Res, end: &Vector2Res, color: &ColorRes) {
        unsafe { rl_draw_line_v(start.inner, end.inner, color.inner) };
    }

    #[ksl_native_export("DrawLineEx")]
    fn draw_line_ex(start: &Vector2Res, end: &Vector2Res, thick: f32, color: &ColorRes) {
        unsafe { rl_draw_line_ex(start.inner, end.inner, thick, color.inner) };
    }

    #[ksl_native_export("DrawLineBezier")]
    fn draw_line_bezier(start: &Vector2Res, end: &Vector2Res, thick: f32, color: &ColorRes) {
        unsafe { rl_draw_line_bezier(start.inner, end.inner, thick, color.inner) };
    }

    #[ksl_native_export("DrawLineDashed")]
    fn draw_line_dashed(
        start: &Vector2Res,
        end: &Vector2Res,
        dash: i32,
        space: i32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_line_dashed(start.inner, end.inner, dash, space, color.inner) };
    }

    /// 填充三角形; 顶点按屏幕坐标的逆时针顺序给出, 否则被背面剔除.
    #[ksl_native_export("DrawTriangle")]
    fn draw_triangle(v1: &Vector2Res, v2: &Vector2Res, v3: &Vector2Res, color: &ColorRes) {
        unsafe { rl_draw_triangle(v1.inner, v2.inner, v3.inner, color.inner) };
    }

    /// 顶点颜色渐变的填充三角形, 顶点顺序要求同 `DrawTriangle`.
    #[ksl_native_export("DrawTriangleGradient")]
    fn draw_triangle_gradient(
        v1: &Vector2Res,
        v2: &Vector2Res,
        v3: &Vector2Res,
        c1: &ColorRes,
        c2: &ColorRes,
        c3: &ColorRes,
    ) {
        unsafe {
            rl_draw_triangle_gradient(
                v1.inner, v2.inner, v3.inner, c1.inner, c2.inner, c3.inner,
            );
        }
    }

    #[ksl_native_export("DrawTriangleLines")]
    fn draw_triangle_lines(
        v1: &Vector2Res,
        v2: &Vector2Res,
        v3: &Vector2Res,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_triangle_lines(v1.inner, v2.inner, v3.inner, color.inner) };
    }

    /// 填充方式实现的粗轮廓三角形, 顶点顺序要求同 `DrawTriangle`.
    #[ksl_native_export("DrawTriangleLinesEx")]
    fn draw_triangle_lines_ex(
        v1: &Vector2Res,
        v2: &Vector2Res,
        v3: &Vector2Res,
        thick: f32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_triangle_lines_ex(v1.inner, v2.inner, v3.inner, thick, color.inner) };
    }

    #[ksl_native_export("DrawRectangle")]
    fn draw_rectangle(x: i32, y: i32, width: i32, height: i32, color: &ColorRes) {
        unsafe { rl_draw_rectangle(x, y, width, height, color.inner) };
    }

    #[ksl_native_export("DrawRectangleV")]
    fn draw_rectangle_v(position: &Vector2Res, size: &Vector2Res, color: &ColorRes) {
        unsafe { rl_draw_rectangle_v(position.inner, size.inner, color.inner) };
    }

    #[ksl_native_export("DrawRectangleRec")]
    fn draw_rectangle_rec(rec: &RectangleRes, color: &ColorRes) {
        unsafe { rl_draw_rectangle_rec(rec.inner, color.inner) };
    }

    #[ksl_native_export("DrawRectangleGradientV")]
    fn draw_rectangle_gradient_v(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        top: &ColorRes,
        bottom: &ColorRes,
    ) {
        unsafe { rl_draw_rectangle_gradient_v(x, y, width, height, top.inner, bottom.inner) };
    }

    #[ksl_native_export("DrawRectangleGradientH")]
    fn draw_rectangle_gradient_h(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        left: &ColorRes,
        right: &ColorRes,
    ) {
        unsafe { rl_draw_rectangle_gradient_h(x, y, width, height, left.inner, right.inner) };
    }

    /// 四角渐变, 颜色按左上, 左下, 右下, 右上给出.
    #[ksl_native_export("DrawRectangleGradientEx")]
    fn draw_rectangle_gradient_ex(
        rec: &RectangleRes,
        top_left: &ColorRes,
        bottom_left: &ColorRes,
        bottom_right: &ColorRes,
        top_right: &ColorRes,
    ) {
        unsafe {
            rl_draw_rectangle_gradient_ex(
                rec.inner,
                top_left.inner,
                bottom_left.inner,
                bottom_right.inner,
                top_right.inner,
            );
        }
    }

    #[ksl_native_export("DrawRectangleLines")]
    fn draw_rectangle_lines(x: i32, y: i32, width: i32, height: i32, color: &ColorRes) {
        unsafe { rl_draw_rectangle_lines(x, y, width, height, color.inner) };
    }

    #[ksl_native_export("DrawRectangleLinesEx")]
    fn draw_rectangle_lines_ex(rec: &RectangleRes, thick: f32, color: &ColorRes) {
        unsafe { rl_draw_rectangle_lines_ex(rec.inner, thick, color.inner) };
    }

    /// 绘制可旋转矩形.
    ///
    /// `origin` 是旋转中心相对矩形左上角的偏移, 矩形的 `x` 与 `y` 是旋转中心的落点;
    /// 即实际左上角为 `(x - origin.x, y - origin.y)`.
    #[ksl_native_export("DrawRectanglePro")]
    fn draw_rectangle_pro(
        rec: &RectangleRes,
        origin: &Vector2Res,
        rotation: f32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_rectangle_pro(rec.inner, origin.inner, rotation, color.inner) };
    }

    #[ksl_native_export("DrawRectangleRounded")]
    fn draw_rectangle_rounded(
        rec: &RectangleRes,
        roundness: f32,
        segments: i32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_rectangle_rounded(rec.inner, roundness, segments, color.inner) };
    }

    #[ksl_native_export("DrawRectangleRoundedLines")]
    fn draw_rectangle_rounded_lines(
        rec: &RectangleRes,
        roundness: f32,
        segments: i32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_rectangle_rounded_lines(rec.inner, roundness, segments, color.inner) };
    }

    #[ksl_native_export("DrawRectangleRoundedLinesEx")]
    fn draw_rectangle_rounded_lines_ex(
        rec: &RectangleRes,
        roundness: f32,
        segments: i32,
        thick: f32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_rectangle_rounded_lines_ex(
                rec.inner,
                roundness,
                segments,
                thick,
                color.inner,
            );
        }
    }

    #[ksl_native_export("DrawPoly")]
    fn draw_poly(
        center: &Vector2Res,
        sides: i32,
        radius: f32,
        rotation: f32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_poly(center.inner, sides, radius, rotation, color.inner) };
    }

    #[ksl_native_export("DrawPolyLines")]
    fn draw_poly_lines(
        center: &Vector2Res,
        sides: i32,
        radius: f32,
        rotation: f32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_poly_lines(center.inner, sides, radius, rotation, color.inner) };
    }

    #[ksl_native_export("DrawPolyLinesEx")]
    fn draw_poly_lines_ex(
        center: &Vector2Res,
        sides: i32,
        radius: f32,
        rotation: f32,
        thick: f32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_poly_lines_ex(center.inner, sides, radius, rotation, thick, color.inner);
        }
    }

    #[ksl_native_export("DrawCircle")]
    fn draw_circle(center_x: i32, center_y: i32, radius: f32, color: &ColorRes) {
        unsafe { rl_draw_circle(center_x, center_y, radius, color.inner) };
    }

    #[ksl_native_export("DrawCircleV")]
    fn draw_circle_v(center: &Vector2Res, radius: f32, color: &ColorRes) {
        unsafe { rl_draw_circle_v(center.inner, radius, color.inner) };
    }

    #[ksl_native_export("DrawCircleGradient")]
    fn draw_circle_gradient(
        center: &Vector2Res,
        radius: f32,
        inner: &ColorRes,
        outer: &ColorRes,
    ) {
        unsafe { rl_draw_circle_gradient(center.inner, radius, inner.inner, outer.inner) };
    }

    #[ksl_native_export("DrawCircleLines")]
    fn draw_circle_lines(center_x: i32, center_y: i32, radius: f32, color: &ColorRes) {
        unsafe { rl_draw_circle_lines(center_x, center_y, radius, color.inner) };
    }

    #[ksl_native_export("DrawCircleLinesV")]
    fn draw_circle_lines_v(center: &Vector2Res, radius: f32, color: &ColorRes) {
        unsafe { rl_draw_circle_lines_v(center.inner, radius, color.inner) };
    }

    #[ksl_native_export("DrawCircleLinesEx")]
    fn draw_circle_lines_ex(center: &Vector2Res, radius: f32, thick: f32, color: &ColorRes) {
        unsafe { rl_draw_circle_lines_ex(center.inner, radius, thick, color.inner) };
    }

    #[ksl_native_export("DrawCircleSector")]
    fn draw_circle_sector(
        center: &Vector2Res,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: i32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_circle_sector(
                center.inner,
                radius,
                start_angle,
                end_angle,
                segments,
                color.inner,
            );
        }
    }

    #[ksl_native_export("DrawCircleSectorLines")]
    fn draw_circle_sector_lines(
        center: &Vector2Res,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: i32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_circle_sector_lines(
                center.inner,
                radius,
                start_angle,
                end_angle,
                segments,
                color.inner,
            );
        }
    }

    #[ksl_native_export("DrawCircleSectorLinesEx")]
    fn draw_circle_sector_lines_ex(
        center: &Vector2Res,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: i32,
        thick: f32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_circle_sector_lines_ex(
                center.inner,
                radius,
                start_angle,
                end_angle,
                segments,
                thick,
                color.inner,
            );
        }
    }

    #[ksl_native_export("DrawEllipse")]
    fn draw_ellipse(
        center_x: i32,
        center_y: i32,
        radius_h: f32,
        radius_v: f32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_ellipse(center_x, center_y, radius_h, radius_v, color.inner) };
    }

    #[ksl_native_export("DrawEllipseV")]
    fn draw_ellipse_v(center: &Vector2Res, radius_h: f32, radius_v: f32, color: &ColorRes) {
        unsafe { rl_draw_ellipse_v(center.inner, radius_h, radius_v, color.inner) };
    }

    #[ksl_native_export("DrawEllipseLines")]
    fn draw_ellipse_lines(
        center_x: i32,
        center_y: i32,
        radius_h: f32,
        radius_v: f32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_ellipse_lines(center_x, center_y, radius_h, radius_v, color.inner) };
    }

    #[ksl_native_export("DrawEllipseLinesV")]
    fn draw_ellipse_lines_v(
        center: &Vector2Res,
        radius_h: f32,
        radius_v: f32,
        color: &ColorRes,
    ) {
        unsafe { rl_draw_ellipse_lines_v(center.inner, radius_h, radius_v, color.inner) };
    }

    #[ksl_native_export("DrawEllipseLinesEx")]
    fn draw_ellipse_lines_ex(
        center: &Vector2Res,
        radius_h: f32,
        radius_v: f32,
        thick: f32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_ellipse_lines_ex(center.inner, radius_h, radius_v, thick, color.inner);
        }
    }

    #[ksl_native_export("DrawRing")]
    fn draw_ring(
        center: &Vector2Res,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: i32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_ring(
                center.inner,
                inner_radius,
                outer_radius,
                start_angle,
                end_angle,
                segments,
                color.inner,
            );
        }
    }

    #[ksl_native_export("DrawRingLines")]
    fn draw_ring_lines(
        center: &Vector2Res,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: i32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_ring_lines(
                center.inner,
                inner_radius,
                outer_radius,
                start_angle,
                end_angle,
                segments,
                color.inner,
            );
        }
    }

    // 参数与 raylib 的 DrawRingLinesEx 一一对应.
    #[expect(clippy::too_many_arguments)]
    #[ksl_native_export("DrawRingLinesEx")]
    fn draw_ring_lines_ex(
        center: &Vector2Res,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: i32,
        thick: f32,
        color: &ColorRes,
    ) {
        unsafe {
            rl_draw_ring_lines_ex(
                center.inner,
                inner_radius,
                outer_radius,
                start_angle,
                end_angle,
                segments,
                thick,
                color.inner,
            );
        }
    }

    #[ksl_native_export("DrawText")]
    fn draw_text(text: &str, x: i32, y: i32, size: i32, color: &ColorRes) {
        let Some(text) = cstring(text) else {
            return;
        };
        unsafe { rl_draw_text(text.as_ptr(), x, y, size, color.inner) };
    }

    #[ksl_native_export("MeasureText")]
    fn measure_text(text: &str, size: i32) -> i32 {
        let Some(text) = cstring(text) else {
            return 0;
        };
        unsafe { rl_measure_text(text.as_ptr(), size) }
    }

    // 颜色
    #[ksl_native_export("Rgba")]
    fn rgba(r: u8, g: u8, b: u8, a: u8) -> ColorRes {
        ColorRes {
            inner: RlColor { r, g, b, a },
        }
    }

    /// 按 `0xRRGGBBAA` 读取颜色.
    #[ksl_native_export("GetColor")]
    fn get_color(hex_value: u32) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_get_color(hex_value) },
        }
    }

    #[ksl_native_export("ColorToInt")]
    fn color_to_int(color: &ColorRes) -> u32 {
        unsafe { rl_color_to_int(color.inner) }
    }

    #[ksl_native_export("ColorIsEqual")]
    fn color_is_equal(left: &ColorRes, right: &ColorRes) -> bool {
        unsafe { rl_color_is_equal(left.inner, right.inner) }
    }

    #[ksl_native_export("Fade")]
    fn fade(color: &ColorRes, alpha: f32) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_fade(color.inner, alpha) },
        }
    }

    /// 色相取 [0, 360], 饱和度与明度取 [0, 1].
    #[ksl_native_export("ColorFromHSV")]
    fn color_from_hsv(hue: f32, saturation: f32, value: f32) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_color_from_hsv(hue, saturation, value) },
        }
    }

    #[ksl_native_export("ColorTint")]
    fn color_tint(color: &ColorRes, tint: &ColorRes) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_color_tint(color.inner, tint.inner) },
        }
    }

    #[ksl_native_export("ColorBrightness")]
    fn color_brightness(color: &ColorRes, factor: f32) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_color_brightness(color.inner, factor) },
        }
    }

    #[ksl_native_export("ColorContrast")]
    fn color_contrast(color: &ColorRes, contrast: f32) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_color_contrast(color.inner, contrast) },
        }
    }

    #[ksl_native_export("ColorAlpha")]
    fn color_alpha(color: &ColorRes, alpha: f32) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_color_alpha(color.inner, alpha) },
        }
    }

    #[ksl_native_export("ColorAlphaBlend")]
    fn color_alpha_blend(dst: &ColorRes, src: &ColorRes, tint: &ColorRes) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_color_alpha_blend(dst.inner, src.inner, tint.inner) },
        }
    }

    #[ksl_native_export("ColorLerp")]
    fn color_lerp(color1: &ColorRes, color2: &ColorRes, factor: f32) -> ColorRes {
        ColorRes {
            inner: unsafe { rl_color_lerp(color1.inner, color2.inner, factor) },
        }
    }

    // 碰撞
    #[ksl_native_export("CheckCollisionRecs")]
    fn check_collision_recs(rec1: &RectangleRes, rec2: &RectangleRes) -> bool {
        unsafe { rl_check_collision_recs(rec1.inner, rec2.inner) }
    }

    #[ksl_native_export("CheckCollisionCircles")]
    fn check_collision_circles(
        center1: &Vector2Res,
        radius1: f32,
        center2: &Vector2Res,
        radius2: f32,
    ) -> bool {
        unsafe { rl_check_collision_circles(center1.inner, radius1, center2.inner, radius2) }
    }

    #[ksl_native_export("CheckCollisionCircleRec")]
    fn check_collision_circle_rec(
        center: &Vector2Res,
        radius: f32,
        rec: &RectangleRes,
    ) -> bool {
        unsafe { rl_check_collision_circle_rec(center.inner, radius, rec.inner) }
    }

    #[ksl_native_export("CheckCollisionCircleLine")]
    fn check_collision_circle_line(
        center: &Vector2Res,
        radius: f32,
        p1: &Vector2Res,
        p2: &Vector2Res,
    ) -> bool {
        unsafe { rl_check_collision_circle_line(center.inner, radius, p1.inner, p2.inner) }
    }

    #[ksl_native_export("CheckCollisionPointRec")]
    fn check_collision_point_rec(point: &Vector2Res, rec: &RectangleRes) -> bool {
        unsafe { rl_check_collision_point_rec(point.inner, rec.inner) }
    }

    #[ksl_native_export("CheckCollisionPointCircle")]
    fn check_collision_point_circle(
        point: &Vector2Res,
        center: &Vector2Res,
        radius: f32,
    ) -> bool {
        unsafe { rl_check_collision_point_circle(point.inner, center.inner, radius) }
    }

    #[ksl_native_export("CheckCollisionPointTriangle")]
    fn check_collision_point_triangle(
        point: &Vector2Res,
        p1: &Vector2Res,
        p2: &Vector2Res,
        p3: &Vector2Res,
    ) -> bool {
        unsafe { rl_check_collision_point_triangle(point.inner, p1.inner, p2.inner, p3.inner) }
    }

    #[ksl_native_export("CheckCollisionPointLine")]
    fn check_collision_point_line(
        point: &Vector2Res,
        p1: &Vector2Res,
        p2: &Vector2Res,
        threshold: i32,
    ) -> bool {
        unsafe { rl_check_collision_point_line(point.inner, p1.inner, p2.inner, threshold) }
    }

    #[ksl_native_export("GetCollisionRec")]
    fn get_collision_rec(rec1: &RectangleRes, rec2: &RectangleRes) -> RectangleRes {
        RectangleRes {
            inner: unsafe { rl_get_collision_rec(rec1.inner, rec2.inner) },
        }
    }

    // 纹理
    #[ksl_native_export("LoadTexture")]
    fn load_texture(path: &str) -> Result<TextureRes, String> {
        let Some(path) = cstring(path) else {
            return Err("纹理路径包含 NUL".to_string());
        };
        let texture = unsafe { rl_load_texture(path.as_ptr()) };
        if texture.id == 0 {
            return Err("无法加载纹理".to_string());
        }
        Ok(TextureRes {
            inner: Handle::new(texture),
        })
    }

    #[ksl_native_export("LoadTextureFromImage")]
    fn load_texture_from_image(image: &ImageRes) -> Result<TextureRes, String> {
        let Some(image) = image.inner.get() else {
            return Err("图像已卸载".to_string());
        };
        let texture = unsafe { rl_load_texture_from_image(image) };
        if texture.id == 0 {
            return Err("无法从图像创建纹理".to_string());
        }
        Ok(TextureRes {
            inner: Handle::new(texture),
        })
    }

    #[ksl_native_export("UnloadTexture")]
    fn unload_texture(texture: &TextureRes) {
        if let Some(inner) = texture.inner.take() {
            unload_texture_safely(inner);
        }
    }

    #[ksl_native_export("IsTextureValid")]
    fn is_texture_valid(texture: &TextureRes) -> bool {
        texture
            .inner
            .get()
            .is_some_and(|inner| unsafe { rl_is_texture_valid(inner) })
    }

    #[ksl_native_export("TextureWidth")]
    fn texture_width(texture: &TextureRes) -> i32 {
        texture.inner.get().map_or(0, |inner| inner.width)
    }

    #[ksl_native_export("TextureHeight")]
    fn texture_height(texture: &TextureRes) -> i32 {
        texture.inner.get().map_or(0, |inner| inner.height)
    }

    #[ksl_native_export("SetTextureFilter")]
    fn set_texture_filter(texture: &TextureRes, filter: i32) {
        if let Some(inner) = texture.inner.get() {
            unsafe { rl_set_texture_filter(inner, filter) };
        }
    }

    /// 用图像当前像素覆盖纹理内容.
    #[ksl_native_export("UpdateTexture")]
    fn update_texture(texture: &TextureRes, image: &ImageRes) {
        let Some(texture) = texture.inner.get() else {
            return;
        };
        let Some(image) = image.inner.get() else {
            return;
        };
        unsafe { rl_update_texture(texture, image.data) };
    }

    /// 用图像内容覆盖纹理的指定矩形区域.
    #[ksl_native_export("UpdateTextureRec")]
    fn update_texture_rec(texture: &TextureRes, rec: &RectangleRes, image: &ImageRes) {
        let Some(texture) = texture.inner.get() else {
            return;
        };
        let Some(image) = image.inner.get() else {
            return;
        };
        unsafe { rl_update_texture_rec(texture, rec.inner, image.data) };
    }

    #[ksl_native_export("SetTextureWrap")]
    fn set_texture_wrap(texture: &TextureRes, wrap: i32) {
        if let Some(inner) = texture.inner.get() {
            unsafe { rl_set_texture_wrap(inner, wrap) };
        }
    }

    #[ksl_native_export("GenTextureMipmaps")]
    fn gen_texture_mipmaps(texture: &TextureRes) {
        let _ = texture
            .inner
            .with_mut(|inner| unsafe { rl_gen_texture_mipmaps(inner) });
    }

    #[ksl_native_export("DrawTexture")]
    fn draw_texture(texture: &TextureRes, x: i32, y: i32, tint: &ColorRes) {
        if let Some(inner) = texture.inner.get() {
            unsafe { rl_draw_texture(inner, x, y, tint.inner) };
        }
    }

    #[ksl_native_export("DrawTextureV")]
    fn draw_texture_v(texture: &TextureRes, position: &Vector2Res, tint: &ColorRes) {
        if let Some(inner) = texture.inner.get() {
            unsafe { rl_draw_texture_v(inner, position.inner, tint.inner) };
        }
    }

    #[ksl_native_export("DrawTextureEx")]
    fn draw_texture_ex(
        texture: &TextureRes,
        position: &Vector2Res,
        rotation: f32,
        scale: f32,
        tint: &ColorRes,
    ) {
        if let Some(inner) = texture.inner.get() {
            unsafe { rl_draw_texture_ex(inner, position.inner, rotation, scale, tint.inner) };
        }
    }

    #[ksl_native_export("DrawTextureRec")]
    fn draw_texture_rec(
        texture: &TextureRes,
        source: &RectangleRes,
        position: &Vector2Res,
        tint: &ColorRes,
    ) {
        if let Some(inner) = texture.inner.get() {
            unsafe { rl_draw_texture_rec(inner, source.inner, position.inner, tint.inner) };
        }
    }

    /// 绘制纹理的一部分到目标矩形, 可旋转.
    ///
    /// `origin` 是旋转中心相对目标矩形左上角的偏移, 目标矩形的 `x` 与 `y` 是旋转中心的落点.
    #[ksl_native_export("DrawTexturePro")]
    fn draw_texture_pro(
        texture: &TextureRes,
        source: &RectangleRes,
        dest: &RectangleRes,
        origin: &Vector2Res,
        rotation: f32,
        tint: &ColorRes,
    ) {
        if let Some(inner) = texture.inner.get() {
            unsafe {
                rl_draw_texture_pro(
                    inner,
                    source.inner,
                    dest.inner,
                    origin.inner,
                    rotation,
                    tint.inner,
                );
            }
        }
    }

    // 图像
    #[ksl_native_export("LoadImage")]
    fn load_image(path: &str) -> Result<ImageRes, String> {
        let Some(path) = cstring(path) else {
            return Err("图像路径包含 NUL".to_string());
        };
        let image = unsafe { rl_load_image(path.as_ptr()) };
        if image.data.is_null() {
            return Err("无法加载图像".to_string());
        }
        Ok(ImageRes {
            inner: Handle::new(image),
        })
    }

    #[ksl_native_export("LoadImageFromTexture")]
    fn load_image_from_texture(texture: &TextureRes) -> Result<ImageRes, String> {
        let Some(texture) = texture.inner.get() else {
            return Err("纹理已卸载".to_string());
        };
        let image = unsafe { rl_load_image_from_texture(texture) };
        if image.data.is_null() {
            return Err("无法从纹理读取图像".to_string());
        }
        Ok(ImageRes {
            inner: Handle::new(image),
        })
    }

    #[ksl_native_export("UnloadImage")]
    fn unload_image(image: &ImageRes) {
        if let Some(inner) = image.inner.take() {
            super::resources::unload_image(inner);
        }
    }

    #[ksl_native_export("IsImageValid")]
    fn is_image_valid(image: &ImageRes) -> bool {
        image
            .inner
            .get()
            .is_some_and(|inner| unsafe { rl_is_image_valid(inner) })
    }

    #[ksl_native_export("ImageWidth")]
    fn image_width(image: &ImageRes) -> i32 {
        image.inner.get().map_or(0, |inner| inner.width)
    }

    #[ksl_native_export("ImageHeight")]
    fn image_height(image: &ImageRes) -> i32 {
        image.inner.get().map_or(0, |inner| inner.height)
    }

    #[ksl_native_export("GenImageColor")]
    fn gen_image_color(width: i32, height: i32, color: &ColorRes) -> ImageRes {
        ImageRes {
            inner: Handle::new(unsafe { rl_gen_image_color(width, height, color.inner) }),
        }
    }

    #[ksl_native_export("GenImageChecked")]
    fn gen_image_checked(
        width: i32,
        height: i32,
        checks_x: i32,
        checks_y: i32,
        col1: &ColorRes,
        col2: &ColorRes,
    ) -> ImageRes {
        ImageRes {
            inner: Handle::new(unsafe {
                rl_gen_image_checked(width, height, checks_x, checks_y, col1.inner, col2.inner)
            }),
        }
    }

    #[ksl_native_export("GetImageColor")]
    fn get_image_color(image: &ImageRes, x: i32, y: i32) -> ColorRes {
        ColorRes {
            inner: image.inner.get().map_or(
                RlColor {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 0,
                },
                |inner| unsafe { rl_get_image_color(inner, x, y) },
            ),
        }
    }

    /// 写入一个像素; 坐标越界时由 raylib 忽略.
    #[ksl_native_export("ImageDrawPixel")]
    fn image_draw_pixel(image: &ImageRes, x: i32, y: i32, color: &ColorRes) {
        image.inner.with_mut(|inner| unsafe {
            rl_image_draw_pixel(inner, x, y, color.inner);
        });
    }

    // 渲染纹理
    #[ksl_native_export("LoadRenderTexture")]
    fn load_render_texture(width: i32, height: i32) -> Result<RenderTextureRes, String> {
        let target = unsafe { rl_load_render_texture(width, height) };
        if target.texture.id == 0 {
            return Err("无法创建渲染纹理".to_string());
        }
        Ok(RenderTextureRes {
            inner: Handle::new(target),
        })
    }

    #[ksl_native_export("UnloadRenderTexture")]
    fn unload_render_texture(target: &RenderTextureRes) {
        if let Some(inner) = target.inner.take() {
            unload_render_texture_safely(inner);
        }
    }

    #[ksl_native_export("BeginTextureMode")]
    fn begin_texture_mode(target: &RenderTextureRes) {
        if let Some(inner) = target.inner.get() {
            unsafe { rl_begin_texture_mode(inner) };
        }
    }

    #[ksl_native_export("EndTextureMode")]
    fn end_texture_mode() {
        unsafe { rl_end_texture_mode() };
    }

    #[ksl_native_export("SetRenderTextureFilter")]
    fn set_render_texture_filter(target: &RenderTextureRes, filter: i32) {
        if let Some(inner) = target.inner.get() {
            unsafe { rl_set_texture_filter(inner.texture, filter) };
        }
    }

    #[ksl_native_export("RenderTextureWidth")]
    fn render_texture_width(target: &RenderTextureRes) -> i32 {
        target.inner.get().map_or(0, |inner| inner.texture.width)
    }

    #[ksl_native_export("RenderTextureHeight")]
    fn render_texture_height(target: &RenderTextureRes) -> i32 {
        target.inner.get().map_or(0, |inner| inner.texture.height)
    }

    /// 绘制整个渲染纹理; 源矩形高度取负, 与 raylib 默认纹理方向一致.
    // 纹理尺寸来自 raylib, 实际远小于 f32 可精确表示的范围.
    #[expect(clippy::cast_precision_loss)]
    #[ksl_native_export("DrawRenderTexture")]
    fn draw_render_texture(target: &RenderTextureRes, position: &Vector2Res, tint: &ColorRes) {
        let Some(inner) = target.inner.get() else {
            return;
        };
        let source = RlRectangle {
            x: 0.0,
            y: 0.0,
            width: inner.texture.width as f32,
            height: -(inner.texture.height as f32),
        };
        unsafe { rl_draw_texture_rec(inner.texture, source, position.inner, tint.inner) };
    }

    // 字体与文本
    #[ksl_native_export("LoadFont")]
    fn load_font(path: &str, size: i32) -> Result<FontRes, String> {
        let Some(path) = cstring(path) else {
            return Err("字体路径包含 NUL".to_string());
        };
        let font = unsafe { rl_load_font_ex(path.as_ptr(), size, std::ptr::null(), 0) };
        if font.texture.id == 0 {
            return Err("无法加载字体".to_string());
        }
        Ok(FontRes::owned(font))
    }

    /// 加载字体, 码点取自 `text` 的全部字符.
    ///
    /// 重复码点只保留第一次出现的位置; 空文本按 raylib 默认字符集加载.
    #[ksl_native_export("LoadFontFromText")]
    fn load_font_from_text(path: &str, size: i32, text: &str) -> Result<FontRes, String> {
        let Some(path) = cstring(path) else {
            return Err("字体路径包含 NUL".to_string());
        };
        let mut codepoints = Vec::new();
        for character in text.chars() {
            let value = character as i32;
            if !codepoints.contains(&value) {
                codepoints.push(value);
            }
        }
        let (pointer, count) = if codepoints.is_empty() {
            (std::ptr::null(), 0)
        } else {
            let count = c_int::try_from(codepoints.len())
                .map_err(|_| "码点数量超出 raylib 上限".to_string())?;
            (codepoints.as_ptr(), count)
        };
        let font = unsafe { rl_load_font_ex(path.as_ptr(), size, pointer, count) };
        if font.texture.id == 0 {
            return Err("无法加载字体".to_string());
        }
        Ok(FontRes::owned(font))
    }

    /// 默认字体由 raylib 持有, 卸载函数对它不生效.
    #[ksl_native_export("GetFontDefault")]
    fn get_font_default() -> FontRes {
        FontRes::borrowed(unsafe { rl_get_font_default() })
    }

    #[ksl_native_export("UnloadFont")]
    fn unload_font(font: &FontRes) {
        if !font.owned {
            return;
        }
        if let Some(inner) = font.inner.take() {
            unload_font_safely(inner);
        }
    }

    #[ksl_native_export("FontBaseSize")]
    fn font_base_size(font: &FontRes) -> i32 {
        font.inner.get().map_or(0, |inner| inner.base_size)
    }

    #[ksl_native_export("FontGlyphCount")]
    fn font_glyph_count(font: &FontRes) -> i32 {
        font.inner.get().map_or(0, |inner| inner.glyph_count)
    }

    #[ksl_native_export("FontAtlasWidth")]
    fn font_atlas_width(font: &FontRes) -> i32 {
        font.inner.get().map_or(0, |inner| inner.texture.width)
    }

    #[ksl_native_export("FontAtlasHeight")]
    fn font_atlas_height(font: &FontRes) -> i32 {
        font.inner.get().map_or(0, |inner| inner.texture.height)
    }

    #[ksl_native_export("SetFontFilter")]
    fn set_font_filter(font: &FontRes, filter: i32) {
        if let Some(inner) = font.inner.get() {
            unsafe { rl_set_texture_filter(inner.texture, filter) };
        }
    }

    #[ksl_native_export("DrawFontAtlas")]
    fn draw_font_atlas(font: &FontRes, x: i32, y: i32, tint: &ColorRes) {
        if let Some(inner) = font.inner.get() {
            unsafe { rl_draw_texture(inner.texture, x, y, tint.inner) };
        }
    }

    /// 绘制可旋转文本.
    ///
    /// `origin` 是旋转中心相对文本左上角的偏移, `position` 是旋转中心的落点.
    // 参数与 raylib 的 DrawTextPro 一一对应.
    #[expect(clippy::too_many_arguments)]
    #[ksl_native_export("DrawTextPro")]
    fn draw_text_pro(
        font: &FontRes,
        text: &str,
        position: &Vector2Res,
        origin: &Vector2Res,
        rotation: f32,
        size: f32,
        spacing: f32,
        tint: &ColorRes,
    ) {
        let Some(inner) = font.inner.get() else {
            return;
        };
        let Some(text) = cstring(text) else {
            return;
        };
        unsafe {
            rl_draw_text_pro(
                inner,
                text.as_ptr(),
                position.inner,
                origin.inner,
                rotation,
                size,
                spacing,
                tint.inner,
            );
        }
    }

    #[ksl_native_export("SetTextLineSpacing")]
    fn set_text_line_spacing(spacing: i32) {
        unsafe { rl_set_text_line_spacing(spacing) };
    }

    #[ksl_native_export("DrawTextEx")]
    fn draw_text_ex(
        font: &FontRes,
        text: &str,
        position: &Vector2Res,
        size: f32,
        spacing: f32,
        tint: &ColorRes,
    ) {
        let Some(inner) = font.inner.get() else {
            return;
        };
        let Some(text) = cstring(text) else {
            return;
        };
        unsafe {
            rl_draw_text_ex(inner, text.as_ptr(), position.inner, size, spacing, tint.inner);
        }
    }

    /// 返回文本的宽与高, 未加载字体时两者都是 0.
    #[ksl_native_export("MeasureTextEx")]
    fn measure_text_ex(font: &FontRes, text: &str, size: f32, spacing: f32) -> Vector2Res {
        let Some(inner) = font.inner.get() else {
            return Vector2Res {
                inner: vector(0.0, 0.0),
            };
        };
        let Some(text) = cstring(text) else {
            return Vector2Res {
                inner: vector(0.0, 0.0),
            };
        };
        Vector2Res {
            inner: unsafe { rl_measure_text_ex(inner, text.as_ptr(), size, spacing) },
        }
    }

    // 码点数组
    #[ksl_native_export("LoadCodepoints")]
    fn load_codepoints(text: &str) -> CodepointsRes {
        let mut codepoints = Vec::new();
        if let Some(text) = cstring(text) {
            let mut count = 0;
            let pointer =
                unsafe { rl_load_codepoints(text.as_ptr(), std::ptr::from_mut(&mut count)) };
            if !pointer.is_null() {
                let count = usize::try_from(count).unwrap_or(0);
                codepoints = unsafe { std::slice::from_raw_parts(pointer, count) }.to_vec();
                unsafe { rl_unload_codepoints(pointer) };
            }
        }
        CodepointsRes {
            data: Handle::new(codepoints),
        }
    }

    #[ksl_native_export("CodepointCount")]
    fn codepoint_count(codepoints: &CodepointsRes) -> u64 {
        codepoints.data.with(|data| data.len() as u64).unwrap_or(0)
    }

    #[ksl_native_export("CodepointAt")]
    fn codepoint_at(codepoints: &CodepointsRes, index: u64) -> u64 {
        let Ok(index) = usize::try_from(index) else {
            return 0;
        };
        codepoints
            .data
            .with(|data| data.get(index).copied())
            .flatten()
            .map_or(0, |value| u64::from(u32::try_from(value).unwrap_or(0)))
    }

    #[ksl_native_export("UnloadCodepoints")]
    fn unload_codepoints(codepoints: &CodepointsRes) {
        codepoints.data.take();
    }

    // Shader
    /// 加载片段着色器; 顶点着色器使用 raylib 默认版本.
    #[ksl_native_export("LoadShaderFromFragment")]
    fn load_shader_from_fragment(path: &str) -> Result<ShaderRes, String> {
        let Some(path) = cstring(path) else {
            return Err("shader 路径包含 NUL".to_string());
        };
        let shader = unsafe { rl_load_shader(std::ptr::null(), path.as_ptr()) };
        if unsafe { rl_is_shader_valid(shader) } {
            Ok(ShaderRes {
                inner: Handle::new(shader),
            })
        } else {
            Err("shader 加载失败".to_string())
        }
    }

    #[ksl_native_export("UnloadShader")]
    fn unload_shader(shader: &ShaderRes) {
        shader.inner.take();
    }

    #[ksl_native_export("IsShaderValid")]
    fn is_shader_valid(shader: &ShaderRes) -> bool {
        shader
            .inner
            .get()
            .is_some_and(|inner| unsafe { rl_is_shader_valid(inner) })
    }

    /// 取 uniform 位置; 未找到或 shader 已卸载时返回 -1.
    #[ksl_native_export("GetShaderLocation")]
    fn get_shader_location(shader: &ShaderRes, name: &str) -> i32 {
        let Some(inner) = shader.inner.get() else {
            return -1;
        };
        let Some(name) = cstring(name) else {
            return -1;
        };
        unsafe { rl_get_shader_location(inner, name.as_ptr()) }
    }

    /// 设置 float uniform.
    #[ksl_native_export("SetShaderValueFloat")]
    fn set_shader_value_float(shader: &ShaderRes, location: i32, value: f32) {
        let Some(inner) = shader.inner.get() else {
            return;
        };
        let data = [value];
        unsafe {
            rl_set_shader_value(
                inner,
                location,
                data.as_ptr().cast::<c_void>(),
                SHADER_UNIFORM_FLOAT,
            );
        }
    }

    /// 设置 vec2 uniform.
    #[ksl_native_export("SetShaderValueVec2")]
    fn set_shader_value_vec2(shader: &ShaderRes, location: i32, value: &Vector2Res) {
        let Some(inner) = shader.inner.get() else {
            return;
        };
        let data = [value.inner.x, value.inner.y];
        unsafe {
            rl_set_shader_value(
                inner,
                location,
                data.as_ptr().cast::<c_void>(),
                SHADER_UNIFORM_VEC2,
            );
        }
    }

    /// 把纹理绑定到 sampler2D uniform.
    #[ksl_native_export("SetShaderValueTexture")]
    fn set_shader_value_texture(shader: &ShaderRes, location: i32, texture: &TextureRes) {
        let Some(shader) = shader.inner.get() else {
            return;
        };
        let Some(texture) = texture.inner.get() else {
            return;
        };
        unsafe { rl_set_shader_value_texture(shader, location, texture) };
    }

    #[ksl_native_export("BeginShaderMode")]
    fn begin_shader_mode(shader: &ShaderRes) {
        if let Some(inner) = shader.inner.get() {
            unsafe { rl_begin_shader_mode(inner) };
        }
    }

    #[ksl_native_export("EndShaderMode")]
    fn end_shader_mode() {
        unsafe { rl_end_shader_mode() };
    }

    // 音频
    /// 加载音频文件; 样本保留 raylib 解码后的格式.
    #[ksl_native_export("LoadWave")]
    fn load_wave(path: &str) -> Result<WaveRes, String> {
        let Some(path) = cstring(path) else {
            return Err("波形路径包含 NUL".to_string());
        };
        let wave = unsafe { rl_load_wave(path.as_ptr()) };
        if unsafe { rl_is_wave_valid(wave) } {
            Ok(WaveRes {
                inner: Handle::new(wave),
            })
        } else {
            Err("无法加载音频文件".to_string())
        }
    }

    #[ksl_native_export("UnloadWave")]
    fn unload_wave(wave: &WaveRes) {
        wave.inner.take();
    }

    #[ksl_native_export("WaveFrameCount")]
    fn wave_frame_count(wave: &WaveRes) -> i32 {
        wave.inner
            .get()
            .map_or(0, |inner| i32::try_from(inner.frame_count).unwrap_or(0))
    }

    #[ksl_native_export("WaveSampleRate")]
    fn wave_sample_rate(wave: &WaveRes) -> i32 {
        wave.inner
            .get()
            .map_or(0, |inner| i32::try_from(inner.sample_rate).unwrap_or(0))
    }

    /// 读取单声道混合样本块; 越过末尾时从头循环.
    #[ksl_native_export("WaveChunk")]
    fn wave_chunk(wave: &WaveRes, start: u64, count: u64) -> Result<Value, String> {
        let Some(inner) = wave.inner.get() else {
            return Err("波形已卸载".to_string());
        };
        if inner.data.is_null() || inner.frame_count == 0 || inner.channels == 0 {
            return Err("波形没有样本".to_string());
        }
        let frames =
            usize::try_from(inner.frame_count).map_err(|_| "帧数超出范围".to_string())?;
        let channels =
            usize::try_from(inner.channels).map_err(|_| "声道数超出范围".to_string())?;
        let count = usize::try_from(count).map_err(|_| "样本数超出范围".to_string())?;
        let start = usize::try_from(start).map_err(|_| "起点超出范围".to_string())? % frames;
        let channel_scale =
            1.0 / f32::from(u16::try_from(channels).map_err(|_| "声道数超出范围".to_string())?);
        let mut samples = Vec::with_capacity(count);
        match inner.sample_size {
            8 => {
                let data = inner.data.cast::<u8>();
                for offset in 0..count {
                    let frame = (start + offset) % frames;
                    let mut sum = 0.0f32;
                    for channel in 0..channels {
                        let value = unsafe { *data.add(frame * channels + channel) };
                        sum += (f32::from(value) - 128.0) / 128.0;
                    }
                    samples.push(sum * channel_scale);
                }
            },
            16 => {
                let data = inner.data.cast::<i16>();
                for offset in 0..count {
                    let frame = (start + offset) % frames;
                    let mut sum = 0.0f32;
                    for channel in 0..channels {
                        let value = unsafe { *data.add(frame * channels + channel) };
                        sum += f32::from(value) / 32768.0;
                    }
                    samples.push(sum * channel_scale);
                }
            },
            32 => {
                let data = inner.data.cast::<f32>();
                for offset in 0..count {
                    let frame = (start + offset) % frames;
                    let mut sum = 0.0f32;
                    for channel in 0..channels {
                        let value = unsafe { *data.add(frame * channels + channel) };
                        sum += value;
                    }
                    samples.push(sum * channel_scale);
                }
            },
            other => return Err(format!("不支持的样本位深 {other}")),
        }
        Ok(Value::list_f32(samples))
    }

    /// 初始化音频设备; 失败时返回错误文本.
    #[ksl_native_export("InitAudioDevice")]
    fn init_audio_device() -> Result<(), String> {
        unsafe { rl_init_audio_device() };
        if unsafe { rl_is_audio_device_ready() } {
            set_audio_open(true);
            Ok(())
        } else {
            Err("音频设备初始化失败".to_string())
        }
    }

    #[ksl_native_export("CloseAudioDevice")]
    fn close_audio_device() {
        unsafe { rl_close_audio_device() };
        set_audio_open(false);
    }

    #[ksl_native_export("IsAudioDeviceReady")]
    fn is_audio_device_ready() -> bool {
        unsafe { rl_is_audio_device_ready() }
    }

    #[ksl_native_export("SetAudioStreamBufferSizeDefault")]
    fn set_audio_stream_buffer_size_default(size: i32) {
        unsafe { rl_set_audio_stream_buffer_size_default(size) };
    }

    /// 创建原始音频流; 样本格式由采样率, 位深与声道数给出.
    #[ksl_native_export("LoadAudioStream")]
    fn load_audio_stream(
        sample_rate: i32,
        sample_size: i32,
        channels: i32,
    ) -> Result<AudioStreamRes, String> {
        let (Ok(sample_rate), Ok(sample_size), Ok(channels)) = (
            u32::try_from(sample_rate),
            u32::try_from(sample_size),
            u32::try_from(channels),
        ) else {
            return Err("音频流参数必须为正整数".to_string());
        };
        if sample_rate == 0 || sample_size == 0 || channels == 0 {
            return Err("音频流参数必须为正整数".to_string());
        }
        if !unsafe { rl_is_audio_device_ready() } {
            return Err("音频设备未初始化".to_string());
        }
        let stream = unsafe { rl_load_audio_stream(sample_rate, sample_size, channels) };
        if unsafe { rl_is_audio_stream_valid(stream) } {
            Ok(AudioStreamRes {
                inner: Handle::new(stream),
            })
        } else {
            Err("音频流创建失败".to_string())
        }
    }

    #[ksl_native_export("UnloadAudioStream")]
    fn unload_audio_stream(stream: &AudioStreamRes) {
        stream.inner.take();
    }

    #[ksl_native_export("IsAudioStreamValid")]
    fn is_audio_stream_valid(stream: &AudioStreamRes) -> bool {
        stream
            .inner
            .get()
            .is_some_and(|inner| unsafe { rl_is_audio_stream_valid(inner) })
    }

    #[ksl_native_export("IsAudioStreamProcessed")]
    fn is_audio_stream_processed(stream: &AudioStreamRes) -> bool {
        stream
            .inner
            .get()
            .is_some_and(|inner| unsafe { rl_is_audio_stream_processed(inner) })
    }

    #[ksl_native_export("IsAudioStreamPlaying")]
    fn is_audio_stream_playing(stream: &AudioStreamRes) -> bool {
        stream
            .inner
            .get()
            .is_some_and(|inner| unsafe { rl_is_audio_stream_playing(inner) })
    }

    /// 写入单声道 f32 样本; 调用方负责让样本数与流格式匹配.
    #[ksl_native_export("UpdateAudioStream")]
    fn update_audio_stream(stream: &AudioStreamRes, samples: &Value) -> Result<(), String> {
        let Some(inner) = stream.inner.get() else {
            return Err("音频流已卸载".to_string());
        };
        let count = samples.list_len().ok_or_else(|| "实参不是列表".to_string())?;
        let mut data = Vec::with_capacity(count);
        for index in 0..count {
            let sample = samples
                .list_get(index)
                .and_then(|value| value.as_f32())
                .ok_or_else(|| "样本不是 f32".to_string())?;
            data.push(sample);
        }
        let frame_count =
            c_int::try_from(data.len()).map_err(|_| "样本数超出范围".to_string())?;
        unsafe {
            rl_update_audio_stream(inner, data.as_ptr().cast::<c_void>(), frame_count);
        }
        Ok(())
    }

    #[ksl_native_export("PlayAudioStream")]
    fn play_audio_stream(stream: &AudioStreamRes) {
        if let Some(inner) = stream.inner.get() {
            unsafe { rl_play_audio_stream(inner) };
        }
    }

    #[ksl_native_export("PauseAudioStream")]
    fn pause_audio_stream(stream: &AudioStreamRes) {
        if let Some(inner) = stream.inner.get() {
            unsafe { rl_pause_audio_stream(inner) };
        }
    }

    #[ksl_native_export("ResumeAudioStream")]
    fn resume_audio_stream(stream: &AudioStreamRes) {
        if let Some(inner) = stream.inner.get() {
            unsafe { rl_resume_audio_stream(inner) };
        }
    }

    #[ksl_native_export("StopAudioStream")]
    fn stop_audio_stream(stream: &AudioStreamRes) {
        if let Some(inner) = stream.inner.get() {
            unsafe { rl_stop_audio_stream(inner) };
        }
    }

    #[ksl_native_export("SetAudioStreamPan")]
    fn set_audio_stream_pan(stream: &AudioStreamRes, pan: f32) {
        if let Some(inner) = stream.inner.get() {
            unsafe { rl_set_audio_stream_pan(inner, pan) };
        }
    }

    #[ksl_native_export("SetAudioStreamVolume")]
    fn set_audio_stream_volume(stream: &AudioStreamRes, volume: f32) {
        if let Some(inner) = stream.inner.get() {
            unsafe { rl_set_audio_stream_volume(inner, volume) };
        }
    }

    #[ksl_native_export("SetAudioStreamPitch")]
    fn set_audio_stream_pitch(stream: &AudioStreamRes, pitch: f32) {
        if let Some(inner) = stream.inner.get() {
            unsafe { rl_set_audio_stream_pitch(inner, pitch) };
        }
    }
}

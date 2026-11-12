//! raylib C 接口与值转换.
//!
//! 结构体布局与 raylib.h 一致; 对外只暴露标量, 字符串与不透明句柄.

use std::ffi::{CString, c_char, c_int, c_uint, c_void};

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlColor {
    pub(crate) r: u8,
    pub(crate) g: u8,
    pub(crate) b: u8,
    pub(crate) a: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlVector2 {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlRectangle {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlTexture2D {
    pub(crate) id: c_uint,
    pub(crate) width: c_int,
    pub(crate) height: c_int,
    pub(crate) mipmaps: c_int,
    pub(crate) format: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlRenderTexture2D {
    pub(crate) id: c_uint,
    pub(crate) texture: RlTexture2D,
    pub(crate) depth: RlTexture2D,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlFont {
    pub(crate) base_size: c_int,
    pub(crate) glyph_count: c_int,
    pub(crate) glyph_padding: c_int,
    pub(crate) texture: RlTexture2D,
    pub(crate) recs: *mut RlRectangle,
    pub(crate) glyphs: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlImage {
    pub(crate) data: *mut c_void,
    pub(crate) width: c_int,
    pub(crate) height: c_int,
    pub(crate) mipmaps: c_int,
    pub(crate) format: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlAudioStream {
    pub(crate) buffer: *mut c_void,
    pub(crate) processor: *mut c_void,
    pub(crate) sample_rate: c_uint,
    pub(crate) sample_size: c_uint,
    pub(crate) channels: c_uint,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlWave {
    pub(crate) frame_count: c_uint,
    pub(crate) sample_rate: c_uint,
    pub(crate) sample_size: c_uint,
    pub(crate) channels: c_uint,
    pub(crate) data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RlShader {
    pub(crate) id: c_uint,
    pub(crate) locs: *mut c_int,
}

/// `SetShaderValue` 的 `SHADER_UNIFORM_FLOAT`.
pub(crate) const SHADER_UNIFORM_FLOAT: c_int = 0;

/// `SetShaderValue` 的 `SHADER_UNIFORM_VEC2`.
pub(crate) const SHADER_UNIFORM_VEC2: c_int = 1;

unsafe extern "C" {
    // 窗口与剪贴板
    #[link_name = "InitWindow"]
    pub(crate) fn rl_init_window(width: c_int, height: c_int, title: *const c_char);
    #[link_name = "CloseWindow"]
    pub(crate) fn rl_close_window();
    #[link_name = "WindowShouldClose"]
    pub(crate) fn rl_window_should_close() -> bool;
    #[link_name = "GetScreenWidth"]
    pub(crate) fn rl_get_screen_width() -> c_int;
    #[link_name = "GetScreenHeight"]
    pub(crate) fn rl_get_screen_height() -> c_int;
    #[link_name = "SetClipboardText"]
    pub(crate) fn rl_set_clipboard_text(text: *const c_char);
    #[link_name = "GetClipboardText"]
    pub(crate) fn rl_get_clipboard_text() -> *const c_char;
    #[link_name = "TakeScreenshot"]
    pub(crate) fn rl_take_screenshot(file_name: *const c_char);

    // 窗口状态
    #[link_name = "IsWindowReady"]
    pub(crate) fn rl_is_window_ready() -> bool;
    #[link_name = "IsWindowFullscreen"]
    pub(crate) fn rl_is_window_fullscreen() -> bool;
    #[link_name = "IsWindowHidden"]
    pub(crate) fn rl_is_window_hidden() -> bool;
    #[link_name = "IsWindowMinimized"]
    pub(crate) fn rl_is_window_minimized() -> bool;
    #[link_name = "IsWindowMaximized"]
    pub(crate) fn rl_is_window_maximized() -> bool;
    #[link_name = "IsWindowFocused"]
    pub(crate) fn rl_is_window_focused() -> bool;
    #[link_name = "IsWindowResized"]
    pub(crate) fn rl_is_window_resized() -> bool;
    #[link_name = "IsWindowState"]
    pub(crate) fn rl_is_window_state(flag: c_uint) -> bool;
    #[link_name = "SetWindowState"]
    pub(crate) fn rl_set_window_state(flags: c_uint);
    #[link_name = "ClearWindowState"]
    pub(crate) fn rl_clear_window_state(flags: c_uint);
    #[link_name = "ToggleFullscreen"]
    pub(crate) fn rl_toggle_fullscreen();
    #[link_name = "MaximizeWindow"]
    pub(crate) fn rl_maximize_window();
    #[link_name = "MinimizeWindow"]
    pub(crate) fn rl_minimize_window();
    #[link_name = "RestoreWindow"]
    pub(crate) fn rl_restore_window();
    #[link_name = "SetWindowIcon"]
    pub(crate) fn rl_set_window_icon(image: RlImage);
    #[link_name = "SetWindowTitle"]
    pub(crate) fn rl_set_window_title(title: *const c_char);
    #[link_name = "SetWindowPosition"]
    pub(crate) fn rl_set_window_position(x: c_int, y: c_int);
    #[link_name = "SetWindowSize"]
    pub(crate) fn rl_set_window_size(width: c_int, height: c_int);
    #[link_name = "SetWindowMinSize"]
    pub(crate) fn rl_set_window_min_size(width: c_int, height: c_int);
    #[link_name = "SetWindowMaxSize"]
    pub(crate) fn rl_set_window_max_size(width: c_int, height: c_int);
    #[link_name = "SetWindowOpacity"]
    pub(crate) fn rl_set_window_opacity(opacity: f32);
    #[link_name = "SetWindowFocused"]
    pub(crate) fn rl_set_window_focused();
    #[link_name = "GetRenderWidth"]
    pub(crate) fn rl_get_render_width() -> c_int;
    #[link_name = "GetRenderHeight"]
    pub(crate) fn rl_get_render_height() -> c_int;
    #[link_name = "GetWindowPosition"]
    pub(crate) fn rl_get_window_position() -> RlVector2;
    #[link_name = "GetWindowScaleDPI"]
    pub(crate) fn rl_get_window_scale_dpi() -> RlVector2;
    #[link_name = "EnableEventWaiting"]
    pub(crate) fn rl_enable_event_waiting();
    #[link_name = "DisableEventWaiting"]
    pub(crate) fn rl_disable_event_waiting();
    #[link_name = "SetExitKey"]
    pub(crate) fn rl_set_exit_key(key: c_int);

    // 显示器
    #[link_name = "GetMonitorCount"]
    pub(crate) fn rl_get_monitor_count() -> c_int;
    #[link_name = "GetCurrentMonitor"]
    pub(crate) fn rl_get_current_monitor() -> c_int;
    #[link_name = "GetMonitorPosition"]
    pub(crate) fn rl_get_monitor_position(monitor: c_int) -> RlVector2;
    #[link_name = "GetMonitorWidth"]
    pub(crate) fn rl_get_monitor_width(monitor: c_int) -> c_int;
    #[link_name = "GetMonitorHeight"]
    pub(crate) fn rl_get_monitor_height(monitor: c_int) -> c_int;
    #[link_name = "GetMonitorPhysicalWidth"]
    pub(crate) fn rl_get_monitor_physical_width(monitor: c_int) -> c_int;
    #[link_name = "GetMonitorPhysicalHeight"]
    pub(crate) fn rl_get_monitor_physical_height(monitor: c_int) -> c_int;
    #[link_name = "GetMonitorRefreshRate"]
    pub(crate) fn rl_get_monitor_refresh_rate(monitor: c_int) -> c_int;
    #[link_name = "GetMonitorName"]
    pub(crate) fn rl_get_monitor_name(monitor: c_int) -> *const c_char;

    // 绘制
    #[link_name = "ClearBackground"]
    pub(crate) fn rl_clear_background(color: RlColor);
    #[link_name = "BeginDrawing"]
    pub(crate) fn rl_begin_drawing();
    #[link_name = "EndDrawing"]
    pub(crate) fn rl_end_drawing();
    #[link_name = "BeginTextureMode"]
    pub(crate) fn rl_begin_texture_mode(target: RlRenderTexture2D);
    #[link_name = "EndTextureMode"]
    pub(crate) fn rl_end_texture_mode();

    // 计时
    #[link_name = "SetTargetFPS"]
    pub(crate) fn rl_set_target_fps(fps: c_int);
    #[link_name = "GetFrameTime"]
    pub(crate) fn rl_get_frame_time() -> f32;
    #[link_name = "GetFPS"]
    pub(crate) fn rl_get_fps() -> c_int;

    // 随机数
    #[link_name = "GetRandomValue"]
    pub(crate) fn rl_get_random_value(min: c_int, max: c_int) -> c_int;

    // 杂项
    #[link_name = "SetConfigFlags"]
    pub(crate) fn rl_set_config_flags(flags: c_uint);
    #[link_name = "SetTraceLogLevel"]
    pub(crate) fn rl_set_trace_log_level(log_level: c_int);
    #[link_name = "GetTime"]
    pub(crate) fn rl_get_time() -> f64;
    #[link_name = "SetRandomSeed"]
    pub(crate) fn rl_set_random_seed(seed: c_uint);

    // 文件系统
    #[link_name = "ChangeDirectory"]
    pub(crate) fn rl_change_directory(dir_path: *const c_char) -> c_int;

    // 键盘
    #[link_name = "IsKeyPressed"]
    pub(crate) fn rl_is_key_pressed(key: c_int) -> bool;
    #[link_name = "IsKeyDown"]
    pub(crate) fn rl_is_key_down(key: c_int) -> bool;
    #[link_name = "IsKeyReleased"]
    pub(crate) fn rl_is_key_released(key: c_int) -> bool;
    #[link_name = "IsKeyUp"]
    pub(crate) fn rl_is_key_up(key: c_int) -> bool;
    #[link_name = "IsKeyPressedRepeat"]
    pub(crate) fn rl_is_key_pressed_repeat(key: c_int) -> bool;
    #[link_name = "GetKeyPressed"]
    pub(crate) fn rl_get_key_pressed() -> c_int;
    #[link_name = "GetKeyName"]
    pub(crate) fn rl_get_key_name(key: c_int) -> *const c_char;
    #[link_name = "GetCharPressed"]
    pub(crate) fn rl_get_char_pressed() -> c_int;

    // 鼠标
    #[link_name = "IsMouseButtonPressed"]
    pub(crate) fn rl_is_mouse_button_pressed(button: c_int) -> bool;
    #[link_name = "IsMouseButtonDown"]
    pub(crate) fn rl_is_mouse_button_down(button: c_int) -> bool;
    #[link_name = "IsMouseButtonReleased"]
    pub(crate) fn rl_is_mouse_button_released(button: c_int) -> bool;
    #[link_name = "IsMouseButtonUp"]
    pub(crate) fn rl_is_mouse_button_up(button: c_int) -> bool;
    #[link_name = "GetMouseX"]
    pub(crate) fn rl_get_mouse_x() -> c_int;
    #[link_name = "GetMouseY"]
    pub(crate) fn rl_get_mouse_y() -> c_int;
    #[link_name = "GetMousePosition"]
    pub(crate) fn rl_get_mouse_position() -> RlVector2;
    #[link_name = "GetMouseDelta"]
    pub(crate) fn rl_get_mouse_delta() -> RlVector2;
    #[link_name = "SetMousePosition"]
    pub(crate) fn rl_set_mouse_position(x: c_int, y: c_int);
    #[link_name = "SetMouseOffset"]
    pub(crate) fn rl_set_mouse_offset(offset_x: c_int, offset_y: c_int);
    #[link_name = "SetMouseScale"]
    pub(crate) fn rl_set_mouse_scale(scale_x: f32, scale_y: f32);
    #[link_name = "GetMouseWheelMove"]
    pub(crate) fn rl_get_mouse_wheel_move() -> f32;
    #[link_name = "GetMouseWheelMoveV"]
    pub(crate) fn rl_get_mouse_wheel_move_v() -> RlVector2;
    #[link_name = "SetMouseCursor"]
    pub(crate) fn rl_set_mouse_cursor(cursor: c_int);

    // 光标
    #[link_name = "ShowCursor"]
    pub(crate) fn rl_show_cursor();
    #[link_name = "HideCursor"]
    pub(crate) fn rl_hide_cursor();
    #[link_name = "IsCursorHidden"]
    pub(crate) fn rl_is_cursor_hidden() -> bool;
    #[link_name = "EnableCursor"]
    pub(crate) fn rl_enable_cursor();
    #[link_name = "DisableCursor"]
    pub(crate) fn rl_disable_cursor();

    // 基本形状
    #[link_name = "DrawPixel"]
    pub(crate) fn rl_draw_pixel(pos_x: c_int, pos_y: c_int, color: RlColor);
    #[link_name = "DrawPixelV"]
    pub(crate) fn rl_draw_pixel_v(position: RlVector2, color: RlColor);
    #[link_name = "DrawLine"]
    pub(crate) fn rl_draw_line(
        start_x: c_int,
        start_y: c_int,
        end_x: c_int,
        end_y: c_int,
        color: RlColor,
    );
    #[link_name = "DrawLineV"]
    pub(crate) fn rl_draw_line_v(start: RlVector2, end: RlVector2, color: RlColor);
    #[link_name = "DrawLineBezier"]
    pub(crate) fn rl_draw_line_bezier(
        start: RlVector2,
        end: RlVector2,
        thick: f32,
        color: RlColor,
    );
    #[link_name = "DrawTriangle"]
    pub(crate) fn rl_draw_triangle(v1: RlVector2, v2: RlVector2, v3: RlVector2, color: RlColor);
    #[link_name = "DrawTriangleGradient"]
    pub(crate) fn rl_draw_triangle_gradient(
        v1: RlVector2,
        v2: RlVector2,
        v3: RlVector2,
        c1: RlColor,
        c2: RlColor,
        c3: RlColor,
    );
    #[link_name = "DrawTriangleLines"]
    pub(crate) fn rl_draw_triangle_lines(
        v1: RlVector2,
        v2: RlVector2,
        v3: RlVector2,
        color: RlColor,
    );
    #[link_name = "DrawTriangleLinesEx"]
    pub(crate) fn rl_draw_triangle_lines_ex(
        v1: RlVector2,
        v2: RlVector2,
        v3: RlVector2,
        thick: f32,
        color: RlColor,
    );
    #[link_name = "DrawRectangleV"]
    pub(crate) fn rl_draw_rectangle_v(position: RlVector2, size: RlVector2, color: RlColor);
    #[link_name = "DrawRectangleRec"]
    pub(crate) fn rl_draw_rectangle_rec(rec: RlRectangle, color: RlColor);
    #[link_name = "DrawRectangleGradientV"]
    pub(crate) fn rl_draw_rectangle_gradient_v(
        pos_x: c_int,
        pos_y: c_int,
        width: c_int,
        height: c_int,
        top: RlColor,
        bottom: RlColor,
    );
    #[link_name = "DrawRectangleGradientH"]
    pub(crate) fn rl_draw_rectangle_gradient_h(
        pos_x: c_int,
        pos_y: c_int,
        width: c_int,
        height: c_int,
        left: RlColor,
        right: RlColor,
    );
    #[link_name = "DrawRectangleGradientEx"]
    pub(crate) fn rl_draw_rectangle_gradient_ex(
        rec: RlRectangle,
        top_left: RlColor,
        bottom_left: RlColor,
        bottom_right: RlColor,
        top_right: RlColor,
    );
    #[link_name = "DrawRectangleRounded"]
    pub(crate) fn rl_draw_rectangle_rounded(
        rec: RlRectangle,
        roundness: f32,
        segments: c_int,
        color: RlColor,
    );
    #[link_name = "DrawRectangleRoundedLines"]
    pub(crate) fn rl_draw_rectangle_rounded_lines(
        rec: RlRectangle,
        roundness: f32,
        segments: c_int,
        color: RlColor,
    );
    #[link_name = "DrawRectangleRoundedLinesEx"]
    pub(crate) fn rl_draw_rectangle_rounded_lines_ex(
        rec: RlRectangle,
        roundness: f32,
        segments: c_int,
        thick: f32,
        color: RlColor,
    );
    #[link_name = "DrawPoly"]
    pub(crate) fn rl_draw_poly(
        center: RlVector2,
        sides: c_int,
        radius: f32,
        rotation: f32,
        color: RlColor,
    );
    #[link_name = "DrawPolyLines"]
    pub(crate) fn rl_draw_poly_lines(
        center: RlVector2,
        sides: c_int,
        radius: f32,
        rotation: f32,
        color: RlColor,
    );
    #[link_name = "DrawPolyLinesEx"]
    pub(crate) fn rl_draw_poly_lines_ex(
        center: RlVector2,
        sides: c_int,
        radius: f32,
        rotation: f32,
        thick: f32,
        color: RlColor,
    );
    #[link_name = "DrawLineEx"]
    pub(crate) fn rl_draw_line_ex(start: RlVector2, end: RlVector2, thick: f32, color: RlColor);
    #[link_name = "DrawLineDashed"]
    pub(crate) fn rl_draw_line_dashed(
        start: RlVector2,
        end: RlVector2,
        dash: c_int,
        space: c_int,
        color: RlColor,
    );
    #[link_name = "DrawRectangle"]
    pub(crate) fn rl_draw_rectangle(
        pos_x: c_int,
        pos_y: c_int,
        width: c_int,
        height: c_int,
        color: RlColor,
    );
    #[link_name = "DrawRectanglePro"]
    pub(crate) fn rl_draw_rectangle_pro(
        rec: RlRectangle,
        origin: RlVector2,
        rotation: f32,
        color: RlColor,
    );
    #[link_name = "DrawRectangleLines"]
    pub(crate) fn rl_draw_rectangle_lines(
        pos_x: c_int,
        pos_y: c_int,
        width: c_int,
        height: c_int,
        color: RlColor,
    );
    #[link_name = "DrawRectangleLinesEx"]
    pub(crate) fn rl_draw_rectangle_lines_ex(rec: RlRectangle, thick: f32, color: RlColor);
    #[link_name = "DrawCircleV"]
    pub(crate) fn rl_draw_circle_v(center: RlVector2, radius: f32, color: RlColor);
    #[link_name = "DrawCircle"]
    pub(crate) fn rl_draw_circle(center_x: c_int, center_y: c_int, radius: f32, color: RlColor);
    #[link_name = "DrawCircleGradient"]
    pub(crate) fn rl_draw_circle_gradient(
        center: RlVector2,
        radius: f32,
        inner: RlColor,
        outer: RlColor,
    );
    #[link_name = "DrawCircleLines"]
    pub(crate) fn rl_draw_circle_lines(
        center_x: c_int,
        center_y: c_int,
        radius: f32,
        color: RlColor,
    );
    #[link_name = "DrawCircleLinesEx"]
    pub(crate) fn rl_draw_circle_lines_ex(
        center: RlVector2,
        radius: f32,
        thick: f32,
        color: RlColor,
    );
    #[link_name = "DrawCircleSector"]
    pub(crate) fn rl_draw_circle_sector(
        center: RlVector2,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: c_int,
        color: RlColor,
    );
    #[link_name = "DrawCircleSectorLines"]
    pub(crate) fn rl_draw_circle_sector_lines(
        center: RlVector2,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: c_int,
        color: RlColor,
    );
    #[link_name = "DrawCircleLinesV"]
    pub(crate) fn rl_draw_circle_lines_v(center: RlVector2, radius: f32, color: RlColor);
    #[link_name = "DrawCircleSectorLinesEx"]
    pub(crate) fn rl_draw_circle_sector_lines_ex(
        center: RlVector2,
        radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: c_int,
        thick: f32,
        color: RlColor,
    );
    #[link_name = "DrawEllipse"]
    pub(crate) fn rl_draw_ellipse(
        center_x: c_int,
        center_y: c_int,
        radius_h: f32,
        radius_v: f32,
        color: RlColor,
    );
    #[link_name = "DrawEllipseV"]
    pub(crate) fn rl_draw_ellipse_v(
        center: RlVector2,
        radius_h: f32,
        radius_v: f32,
        color: RlColor,
    );
    #[link_name = "DrawEllipseLines"]
    pub(crate) fn rl_draw_ellipse_lines(
        center_x: c_int,
        center_y: c_int,
        radius_h: f32,
        radius_v: f32,
        color: RlColor,
    );
    #[link_name = "DrawEllipseLinesV"]
    pub(crate) fn rl_draw_ellipse_lines_v(
        center: RlVector2,
        radius_h: f32,
        radius_v: f32,
        color: RlColor,
    );
    #[link_name = "DrawEllipseLinesEx"]
    pub(crate) fn rl_draw_ellipse_lines_ex(
        center: RlVector2,
        radius_h: f32,
        radius_v: f32,
        thick: f32,
        color: RlColor,
    );
    #[link_name = "DrawRing"]
    pub(crate) fn rl_draw_ring(
        center: RlVector2,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: c_int,
        color: RlColor,
    );
    #[link_name = "DrawRingLines"]
    pub(crate) fn rl_draw_ring_lines(
        center: RlVector2,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: c_int,
        color: RlColor,
    );
    #[link_name = "DrawRingLinesEx"]
    pub(crate) fn rl_draw_ring_lines_ex(
        center: RlVector2,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        end_angle: f32,
        segments: c_int,
        thick: f32,
        color: RlColor,
    );

    // 碰撞
    #[link_name = "CheckCollisionRecs"]
    pub(crate) fn rl_check_collision_recs(rec1: RlRectangle, rec2: RlRectangle) -> bool;
    #[link_name = "CheckCollisionCircles"]
    pub(crate) fn rl_check_collision_circles(
        center1: RlVector2,
        radius1: f32,
        center2: RlVector2,
        radius2: f32,
    ) -> bool;
    #[link_name = "CheckCollisionCircleRec"]
    pub(crate) fn rl_check_collision_circle_rec(
        center: RlVector2,
        radius: f32,
        rec: RlRectangle,
    ) -> bool;
    #[link_name = "CheckCollisionCircleLine"]
    pub(crate) fn rl_check_collision_circle_line(
        center: RlVector2,
        radius: f32,
        p1: RlVector2,
        p2: RlVector2,
    ) -> bool;
    #[link_name = "CheckCollisionPointRec"]
    pub(crate) fn rl_check_collision_point_rec(point: RlVector2, rec: RlRectangle) -> bool;
    #[link_name = "CheckCollisionPointCircle"]
    pub(crate) fn rl_check_collision_point_circle(
        point: RlVector2,
        center: RlVector2,
        radius: f32,
    ) -> bool;
    #[link_name = "CheckCollisionPointTriangle"]
    pub(crate) fn rl_check_collision_point_triangle(
        point: RlVector2,
        p1: RlVector2,
        p2: RlVector2,
        p3: RlVector2,
    ) -> bool;
    #[link_name = "CheckCollisionPointLine"]
    pub(crate) fn rl_check_collision_point_line(
        point: RlVector2,
        p1: RlVector2,
        p2: RlVector2,
        threshold: c_int,
    ) -> bool;
    #[link_name = "GetCollisionRec"]
    pub(crate) fn rl_get_collision_rec(rec1: RlRectangle, rec2: RlRectangle) -> RlRectangle;

    // 纹理
    #[link_name = "LoadTexture"]
    pub(crate) fn rl_load_texture(file_name: *const c_char) -> RlTexture2D;
    #[link_name = "LoadTextureFromImage"]
    pub(crate) fn rl_load_texture_from_image(image: RlImage) -> RlTexture2D;
    #[link_name = "UpdateTexture"]
    pub(crate) fn rl_update_texture(texture: RlTexture2D, pixels: *const c_void);
    #[link_name = "UpdateTextureRec"]
    pub(crate) fn rl_update_texture_rec(
        texture: RlTexture2D,
        rec: RlRectangle,
        pixels: *const c_void,
    );
    #[link_name = "IsTextureValid"]
    pub(crate) fn rl_is_texture_valid(texture: RlTexture2D) -> bool;
    #[link_name = "UnloadTexture"]
    pub(crate) fn rl_unload_texture(texture: RlTexture2D);
    #[link_name = "GenTextureMipmaps"]
    pub(crate) fn rl_gen_texture_mipmaps(texture: *mut RlTexture2D);
    #[link_name = "SetTextureWrap"]
    pub(crate) fn rl_set_texture_wrap(texture: RlTexture2D, wrap: c_int);
    #[link_name = "DrawTextureV"]
    pub(crate) fn rl_draw_texture_v(texture: RlTexture2D, position: RlVector2, tint: RlColor);
    #[link_name = "DrawTextureEx"]
    pub(crate) fn rl_draw_texture_ex(
        texture: RlTexture2D,
        position: RlVector2,
        rotation: f32,
        scale: f32,
        tint: RlColor,
    );
    #[link_name = "DrawTexturePro"]
    pub(crate) fn rl_draw_texture_pro(
        texture: RlTexture2D,
        source: RlRectangle,
        dest: RlRectangle,
        origin: RlVector2,
        rotation: f32,
        tint: RlColor,
    );
    #[link_name = "LoadRenderTexture"]
    pub(crate) fn rl_load_render_texture(width: c_int, height: c_int) -> RlRenderTexture2D;
    #[link_name = "UnloadRenderTexture"]
    pub(crate) fn rl_unload_render_texture(target: RlRenderTexture2D);
    #[link_name = "SetTextureFilter"]
    pub(crate) fn rl_set_texture_filter(texture: RlTexture2D, filter: c_int);
    #[link_name = "DrawTexture"]
    pub(crate) fn rl_draw_texture(
        texture: RlTexture2D,
        pos_x: c_int,
        pos_y: c_int,
        tint: RlColor,
    );
    #[link_name = "DrawTextureRec"]
    pub(crate) fn rl_draw_texture_rec(
        texture: RlTexture2D,
        source: RlRectangle,
        position: RlVector2,
        tint: RlColor,
    );

    // 图像
    #[link_name = "LoadImage"]
    pub(crate) fn rl_load_image(file_name: *const c_char) -> RlImage;
    #[link_name = "LoadImageFromTexture"]
    pub(crate) fn rl_load_image_from_texture(texture: RlTexture2D) -> RlImage;
    #[link_name = "IsImageValid"]
    pub(crate) fn rl_is_image_valid(image: RlImage) -> bool;
    #[link_name = "UnloadImage"]
    pub(crate) fn rl_unload_image(image: RlImage);
    #[link_name = "GenImageColor"]
    pub(crate) fn rl_gen_image_color(width: c_int, height: c_int, color: RlColor) -> RlImage;
    #[link_name = "GenImageChecked"]
    pub(crate) fn rl_gen_image_checked(
        width: c_int,
        height: c_int,
        checks_x: c_int,
        checks_y: c_int,
        col1: RlColor,
        col2: RlColor,
    ) -> RlImage;
    #[link_name = "GetImageColor"]
    pub(crate) fn rl_get_image_color(image: RlImage, x: c_int, y: c_int) -> RlColor;
    #[link_name = "ImageDrawPixel"]
    pub(crate) fn rl_image_draw_pixel(image: *mut RlImage, x: c_int, y: c_int, color: RlColor);

    // 颜色
    #[link_name = "ColorIsEqual"]
    pub(crate) fn rl_color_is_equal(left: RlColor, right: RlColor) -> bool;
    #[link_name = "Fade"]
    pub(crate) fn rl_fade(color: RlColor, alpha: f32) -> RlColor;
    #[link_name = "ColorToInt"]
    pub(crate) fn rl_color_to_int(color: RlColor) -> c_uint;
    #[link_name = "ColorFromHSV"]
    pub(crate) fn rl_color_from_hsv(hue: f32, saturation: f32, value: f32) -> RlColor;
    #[link_name = "ColorTint"]
    pub(crate) fn rl_color_tint(color: RlColor, tint: RlColor) -> RlColor;
    #[link_name = "ColorBrightness"]
    pub(crate) fn rl_color_brightness(color: RlColor, factor: f32) -> RlColor;
    #[link_name = "ColorContrast"]
    pub(crate) fn rl_color_contrast(color: RlColor, contrast: f32) -> RlColor;
    #[link_name = "ColorAlpha"]
    pub(crate) fn rl_color_alpha(color: RlColor, alpha: f32) -> RlColor;
    #[link_name = "ColorAlphaBlend"]
    pub(crate) fn rl_color_alpha_blend(dst: RlColor, src: RlColor, tint: RlColor) -> RlColor;
    #[link_name = "ColorLerp"]
    pub(crate) fn rl_color_lerp(color1: RlColor, color2: RlColor, factor: f32) -> RlColor;
    #[link_name = "GetColor"]
    pub(crate) fn rl_get_color(hex_value: c_uint) -> RlColor;

    // 文本
    #[link_name = "GetFontDefault"]
    pub(crate) fn rl_get_font_default() -> RlFont;
    #[link_name = "LoadFontEx"]
    pub(crate) fn rl_load_font_ex(
        file_name: *const c_char,
        font_size: c_int,
        codepoints: *const c_int,
        codepoint_count: c_int,
    ) -> RlFont;
    #[link_name = "UnloadFont"]
    pub(crate) fn rl_unload_font(font: RlFont);
    #[link_name = "DrawFPS"]
    pub(crate) fn rl_draw_fps(pos_x: c_int, pos_y: c_int);
    #[link_name = "DrawText"]
    pub(crate) fn rl_draw_text(
        text: *const c_char,
        pos_x: c_int,
        pos_y: c_int,
        font_size: c_int,
        color: RlColor,
    );
    #[link_name = "DrawTextEx"]
    pub(crate) fn rl_draw_text_ex(
        font: RlFont,
        text: *const c_char,
        position: RlVector2,
        font_size: f32,
        spacing: f32,
        tint: RlColor,
    );
    #[link_name = "DrawTextPro"]
    pub(crate) fn rl_draw_text_pro(
        font: RlFont,
        text: *const c_char,
        position: RlVector2,
        origin: RlVector2,
        rotation: f32,
        font_size: f32,
        spacing: f32,
        tint: RlColor,
    );
    #[link_name = "SetTextLineSpacing"]
    pub(crate) fn rl_set_text_line_spacing(spacing: c_int);
    #[link_name = "MeasureText"]
    pub(crate) fn rl_measure_text(text: *const c_char, font_size: c_int) -> c_int;
    #[link_name = "MeasureTextEx"]
    pub(crate) fn rl_measure_text_ex(
        font: RlFont,
        text: *const c_char,
        font_size: f32,
        spacing: f32,
    ) -> RlVector2;

    // 码点
    #[link_name = "LoadCodepoints"]
    pub(crate) fn rl_load_codepoints(text: *const c_char, count: *mut c_int) -> *mut c_int;
    #[link_name = "UnloadCodepoints"]
    pub(crate) fn rl_unload_codepoints(codepoints: *mut c_int);

    // 音频
    #[link_name = "LoadWave"]
    pub(crate) fn rl_load_wave(file_name: *const c_char) -> RlWave;
    #[link_name = "IsWaveValid"]
    pub(crate) fn rl_is_wave_valid(wave: RlWave) -> bool;
    #[link_name = "UnloadWave"]
    pub(crate) fn rl_unload_wave(wave: RlWave);
    #[link_name = "InitAudioDevice"]
    pub(crate) fn rl_init_audio_device();
    #[link_name = "CloseAudioDevice"]
    pub(crate) fn rl_close_audio_device();
    #[link_name = "IsAudioDeviceReady"]
    pub(crate) fn rl_is_audio_device_ready() -> bool;
    #[link_name = "SetAudioStreamBufferSizeDefault"]
    pub(crate) fn rl_set_audio_stream_buffer_size_default(size: c_int);
    #[link_name = "LoadAudioStream"]
    pub(crate) fn rl_load_audio_stream(
        sample_rate: c_uint,
        sample_size: c_uint,
        channels: c_uint,
    ) -> RlAudioStream;
    #[link_name = "IsAudioStreamValid"]
    pub(crate) fn rl_is_audio_stream_valid(stream: RlAudioStream) -> bool;
    #[link_name = "UnloadAudioStream"]
    pub(crate) fn rl_unload_audio_stream(stream: RlAudioStream);
    #[link_name = "UpdateAudioStream"]
    pub(crate) fn rl_update_audio_stream(
        stream: RlAudioStream,
        data: *const c_void,
        frame_count: c_int,
    );
    #[link_name = "IsAudioStreamProcessed"]
    pub(crate) fn rl_is_audio_stream_processed(stream: RlAudioStream) -> bool;
    #[link_name = "PlayAudioStream"]
    pub(crate) fn rl_play_audio_stream(stream: RlAudioStream);
    #[link_name = "PauseAudioStream"]
    pub(crate) fn rl_pause_audio_stream(stream: RlAudioStream);
    #[link_name = "ResumeAudioStream"]
    pub(crate) fn rl_resume_audio_stream(stream: RlAudioStream);
    #[link_name = "IsAudioStreamPlaying"]
    pub(crate) fn rl_is_audio_stream_playing(stream: RlAudioStream) -> bool;
    #[link_name = "StopAudioStream"]
    pub(crate) fn rl_stop_audio_stream(stream: RlAudioStream);
    #[link_name = "SetAudioStreamVolume"]
    pub(crate) fn rl_set_audio_stream_volume(stream: RlAudioStream, volume: f32);
    #[link_name = "SetAudioStreamPitch"]
    pub(crate) fn rl_set_audio_stream_pitch(stream: RlAudioStream, pitch: f32);
    #[link_name = "SetAudioStreamPan"]
    pub(crate) fn rl_set_audio_stream_pan(stream: RlAudioStream, pan: f32);

    // Shader
    #[link_name = "LoadShader"]
    pub(crate) fn rl_load_shader(
        vertex_file_name: *const c_char,
        fragment_file_name: *const c_char,
    ) -> RlShader;
    #[link_name = "IsShaderValid"]
    pub(crate) fn rl_is_shader_valid(shader: RlShader) -> bool;
    #[link_name = "UnloadShader"]
    pub(crate) fn rl_unload_shader(shader: RlShader);
    #[link_name = "GetShaderLocation"]
    pub(crate) fn rl_get_shader_location(
        shader: RlShader,
        uniform_name: *const c_char,
    ) -> c_int;
    #[link_name = "SetShaderValue"]
    pub(crate) fn rl_set_shader_value(
        shader: RlShader,
        location: c_int,
        value: *const c_void,
        uniform_type: c_int,
    );
    #[link_name = "SetShaderValueTexture"]
    pub(crate) fn rl_set_shader_value_texture(
        shader: RlShader,
        location: c_int,
        texture: RlTexture2D,
    );
    #[link_name = "BeginShaderMode"]
    pub(crate) fn rl_begin_shader_mode(shader: RlShader);
    #[link_name = "EndShaderMode"]
    pub(crate) fn rl_end_shader_mode();
}

pub(crate) fn vector(x: f32, y: f32) -> RlVector2 {
    RlVector2 { x, y }
}

pub(crate) fn rectangle(x: f32, y: f32, width: f32, height: f32) -> RlRectangle {
    RlRectangle {
        x,
        y,
        width,
        height,
    }
}

pub(crate) fn cstring(text: &str) -> Option<CString> {
    CString::new(text).ok()
}

//! 插件不透明资源.
//!
//! 每个资源在最后一个 ksl 引用消失时释放; 显式卸载函数与释放入口共享同一状态.
//! 拥有句柄用 `Mutex` 保护, 显式卸载后访问变为空操作.

use std::ffi::c_int;
use std::sync::atomic::{AtomicBool, Ordering};

use ksl_value::value::NativeOpaque;
use parking_lot::Mutex;

use super::ffi::{
    RlAudioStream, RlColor, RlFont, RlImage, RlRectangle, RlRenderTexture2D, RlShader,
    RlTexture2D, RlVector2, RlWave, rl_unload_audio_stream, rl_unload_font, rl_unload_image,
    rl_unload_render_texture, rl_unload_shader, rl_unload_texture, rl_unload_wave,
};

/// 窗口是否处于打开状态.
///
/// raylib 的卸载函数会操作 OpenGL 上下文; 窗口关闭后不调用.
static WINDOW_OPEN: AtomicBool = AtomicBool::new(false);

/// 音频设备是否处于打开状态.
///
/// 音频流在设备关闭后不再调用 raylib 的卸载函数.
static AUDIO_OPEN: AtomicBool = AtomicBool::new(false);

pub(crate) fn set_window_open(open: bool) {
    WINDOW_OPEN.store(open, Ordering::SeqCst);
}

fn window_open() -> bool {
    WINDOW_OPEN.load(Ordering::SeqCst)
}

pub(crate) fn set_audio_open(open: bool) {
    AUDIO_OPEN.store(open, Ordering::SeqCst);
}

fn audio_open() -> bool {
    AUDIO_OPEN.load(Ordering::SeqCst)
}

pub(crate) fn unload_font_safely(font: RlFont) {
    if window_open() {
        unsafe { rl_unload_font(font) };
    }
}

pub(crate) fn unload_render_texture_safely(target: RlRenderTexture2D) {
    if window_open() {
        unsafe { rl_unload_render_texture(target) };
    }
}

pub(crate) fn unload_texture_safely(texture: RlTexture2D) {
    if window_open() {
        unsafe { rl_unload_texture(texture) };
    }
}

pub(crate) fn unload_audio_stream_safely(stream: RlAudioStream) {
    if audio_open() {
        unsafe { rl_unload_audio_stream(stream) };
    }
}

pub(crate) fn unload_shader_safely(shader: RlShader) {
    if window_open() {
        unsafe { rl_unload_shader(shader) };
    }
}

/// 图像驻留 CPU 内存, 窗口关闭后仍可释放.
pub(crate) fn unload_image(image: RlImage) {
    unsafe { rl_unload_image(image) };
}

/// 波形驻留 CPU 内存, 只释放解码后的样本.
pub(crate) fn unload_wave(wave: RlWave) {
    unsafe { rl_unload_wave(wave) };
}

/// 拥有句柄.
///
/// 读取复制的 raylib 结构体; 显式卸载后内部为空, 访问返回 `None`.
pub(crate) struct Handle<T> {
    inner: Mutex<Option<T>>,
}

impl<T> Handle<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self {
            inner: Mutex::new(Some(inner)),
        }
    }

    /// 读取句柄, 已卸载时为 `None`.
    pub(crate) fn with<R>(&self, read: impl FnOnce(&T) -> R) -> Option<R> {
        self.inner.lock().as_ref().map(read)
    }

    /// 修改句柄, 已卸载时为 `None`.
    pub(crate) fn with_mut<R>(&self, write: impl FnOnce(&mut T) -> R) -> Option<R> {
        self.inner.lock().as_mut().map(write)
    }

    /// 取出句柄, 已卸载时为 `None`.
    pub(crate) fn take(&self) -> Option<T> {
        self.inner.lock().take()
    }
}

impl<T: Copy> Handle<T> {
    /// 复制句柄, 已卸载时为 `None`.
    pub(crate) fn get(&self) -> Option<T> {
        self.with(|inner| *inner)
    }
}

/// 定义拥有资源: 内部状态由 `Handle` 保护, 析构时调用卸载函数.
///
/// raylib 资源只在调度线程访问, 不得跨线程传递; `Mutex` 只排除显式卸载与访问的竞争.
macro_rules! owned_resource {
    ($(#[$meta:meta])* $name:ident, $type_name:literal, $inner:ty, $unload:ident) => {
        $(#[$meta])*
        pub struct $name {
            pub(crate) inner: Handle<$inner>,
        }

        unsafe impl Send for $name {}
        unsafe impl Sync for $name {}

        impl NativeOpaque for $name {
            fn type_name(&self) -> &'static str {
                $type_name
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                if let Some(inner) = self.inner.take() {
                    $unload(inner);
                }
            }
        }
    };
}

owned_resource!(
    /// 波形句柄.
    WaveRes,
    "Wave",
    RlWave,
    unload_wave
);

owned_resource!(
    /// 音频流句柄.
    AudioStreamRes,
    "AudioStream",
    RlAudioStream,
    unload_audio_stream_safely
);

owned_resource!(
    /// Shader 句柄.
    ShaderRes,
    "Shader",
    RlShader,
    unload_shader_safely
);

/// 颜色句柄.
pub struct ColorRes {
    pub(crate) inner: RlColor,
}

impl NativeOpaque for ColorRes {
    fn type_name(&self) -> &'static str {
        "Color"
    }
}

/// 二维向量值.
pub struct Vector2Res {
    pub(crate) inner: RlVector2,
}

impl NativeOpaque for Vector2Res {
    fn type_name(&self) -> &'static str {
        "Vector2"
    }
}

/// 矩形值.
pub struct RectangleRes {
    pub(crate) inner: RlRectangle,
}

impl NativeOpaque for RectangleRes {
    fn type_name(&self) -> &'static str {
        "Rectangle"
    }
}

/// 字体句柄.
///
/// 默认字体由 raylib 持有, 析构与 `UnloadFont` 都不释放它.
pub struct FontRes {
    pub(crate) inner: Handle<RlFont>,
    pub(crate) owned: bool,
}

impl FontRes {
    pub(crate) fn owned(font: RlFont) -> Self {
        Self {
            inner: Handle::new(font),
            owned: true,
        }
    }

    pub(crate) fn borrowed(font: RlFont) -> Self {
        Self {
            inner: Handle::new(font),
            owned: false,
        }
    }
}

owned_resource! {
    /// 渲染纹理句柄.
    RenderTextureRes, "RenderTexture", RlRenderTexture2D, unload_render_texture_safely
}

owned_resource! {
    /// 纹理句柄.
    TextureRes, "Texture", RlTexture2D, unload_texture_safely
}

owned_resource! {
    /// 图像句柄.
    ImageRes, "Image", RlImage, unload_image
}

unsafe impl Send for FontRes {}
unsafe impl Sync for FontRes {}

impl NativeOpaque for FontRes {
    fn type_name(&self) -> &'static str {
        "Font"
    }
}

impl Drop for FontRes {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        if let Some(inner) = self.inner.take() {
            unload_font_safely(inner);
        }
    }
}

/// 码点数组句柄.
pub struct CodepointsRes {
    pub(crate) data: Handle<Vec<c_int>>,
}

impl NativeOpaque for CodepointsRes {
    fn type_name(&self) -> &'static str {
        "Codepoints"
    }
}

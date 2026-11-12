//! 格式化参数解析与数值渲染.
//!
//! 检查器与运行时共用同一套规则; 本层不接触运行时值.

/// 宽度上限; 只影响显示.
const MAX_WIDTH: usize = 256;
/// 精度上限; f32 与 f64 的有效十进制位远少于此.
const MAX_PRECISION: usize = 24;

/// 有限值的默认输出带小数点或指数, 使结果写回源码时仍是浮点字面量.
#[must_use]
pub fn format_float_default(value: f64) -> String {
    let mut text = value.to_string();
    if value.is_finite() && !text.contains(['.', 'e', 'E']) {
        text.push_str(".0");
    }
    text
}

/// f32 的默认输出: 取 f32 的最短往返表示, 不展开为 f64 的精确值.
#[must_use]
pub fn format_float32_default(value: f32) -> String {
    let mut text = value.to_string();
    if value.is_finite() && !text.contains(['.', 'e', 'E']) {
        text.push_str(".0");
    }
    text
}

/// 格式参数适用的值类别.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Int,
    Float,
    Text,
}

/// 校验格式参数是否适用于给定类别; 检查器与运行时共用同一文法.
#[must_use]
pub fn spec_matches(kind: FormatKind, text: &str) -> bool {
    let Ok(spec) = Spec::parse(Some(text)) else {
        return false;
    };
    match kind {
        // 整数不接受精度.
        FormatKind::Int => {
            spec.precision.is_none() && matches!(spec.ty, None | Some('x' | 'X' | 'b'))
        },
        FormatKind::Float => !spec.alternate && matches!(spec.ty, None | Some('e' | 'E')),
        // 文本只接受排版与 `?`; 进制, 符号与精度对它没有意义.
        FormatKind::Text => {
            spec.ty.is_none()
                && spec.precision.is_none()
                && !spec.alternate
                && spec.sign.is_none()
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Right,
    Center,
}

/// 解析后的格式参数; 字段对应文法各部分.
#[derive(Debug, Default)]
pub struct Spec {
    fill: Option<char>,
    align: Option<Align>,
    sign: Option<char>,
    alternate: bool,
    zero: bool,
    width: Option<usize>,
    precision: Option<usize>,
    /// 源码形式: 文本加引号, 数值加类型后缀.
    repr: bool,
    ty: Option<char>,
}

impl Spec {
    /// 解析 Rust 式格式参数子集: `[[fill]align][sign][#][0][width][.precision][type][?]`.
    ///
    /// # Errors
    ///
    /// 格式参数非法或超出宽度与精度上限时返回错误文本.
    pub fn parse(text: Option<&str>) -> Result<Self, String> {
        let Some(text) = text else {
            return Ok(Self::default());
        };
        if text.is_empty() {
            return Err("格式参数为空".to_string());
        }
        let chars: Vec<char> = text.chars().collect();
        let mut pos = 0;
        let mut spec = Self::default();
        // 填充 + 对齐: 两个字符且次位为对齐符.
        if chars.len() >= 2 && matches!(chars[1], '<' | '>' | '^') {
            spec.fill = Some(chars[0]);
            spec.align = Some(parse_align(chars[1]));
            pos = 2;
        } else if pos < chars.len() && matches!(chars[pos], '<' | '>' | '^') {
            spec.align = Some(parse_align(chars[pos]));
            pos += 1;
        }
        if pos < chars.len() && matches!(chars[pos], '+' | '-') {
            spec.sign = Some(chars[pos]);
            pos += 1;
        }
        if pos < chars.len() && chars[pos] == '#' {
            spec.alternate = true;
            pos += 1;
        }
        if pos < chars.len() && chars[pos] == '0' {
            spec.zero = true;
            pos += 1;
        }
        let width_start = pos;
        while pos < chars.len() && chars[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos > width_start {
            let digits: String = chars[width_start..pos].iter().collect();
            let width = digits
                .parse::<usize>()
                .map_err(|_| format!("格式宽度非法: {text}"))?;
            if width > MAX_WIDTH {
                return Err(format!("格式宽度超出上限: {text}"));
            }
            spec.width = Some(width);
        }
        if pos < chars.len() && chars[pos] == '.' {
            pos += 1;
            let precision_start = pos;
            while pos < chars.len() && chars[pos].is_ascii_digit() {
                pos += 1;
            }
            let digits: String = chars[precision_start..pos].iter().collect();
            let precision = digits
                .parse::<usize>()
                .map_err(|_| format!("格式精度非法: {text}"))?;
            if precision > MAX_PRECISION {
                return Err(format!("格式精度超出上限: {text}"));
            }
            spec.precision = Some(precision);
        }
        if pos < chars.len() && chars[pos] != '?' {
            spec.ty = Some(chars[pos]);
            pos += 1;
        }
        // `?` 是类型字符之后的独立修饰位, 可与宽度等排版组合.
        if pos < chars.len() && chars[pos] == '?' {
            spec.repr = true;
            pos += 1;
        }
        if pos != chars.len() {
            return Err(format!("格式参数非法: {text}"));
        }
        // 类型字符限定为运行时实际支持的集合; 不支持八进制.
        if !matches!(spec.ty, None | Some('x' | 'X' | 'b' | 'e' | 'E')) {
            return Err(format!("不支持的格式类型: {:?}", spec.ty));
        }
        Ok(spec)
    }

    /// 是否要求源码形式.
    #[must_use]
    pub fn repr(&self) -> bool {
        self.repr
    }

    /// 渲染整数: 进制, 符号, 类型后缀与宽度填充.
    ///
    /// 类型后缀计入宽度; 整数不接受精度.
    ///
    /// # Errors
    ///
    /// 整数带精度时返回错误文本.
    pub fn apply_int(
        &self,
        magnitude: i128,
        negative: bool,
        suffix: &str,
    ) -> Result<String, String> {
        if self.precision.is_some() {
            return Err("整数不接受精度".to_string());
        }
        let digits = match self.ty {
            Some('x') => format!("{:x}", magnitude.unsigned_abs()),
            Some('X') => format!("{:X}", magnitude.unsigned_abs()),
            Some('b') => format!("{:b}", magnitude.unsigned_abs()),
            _ => magnitude.unsigned_abs().to_string(),
        };
        let prefixed = if self.alternate {
            match self.ty {
                // 前缀恒为小写, `X` 只改变数字的大小写.
                Some('x' | 'X') => format!("0x{digits}"),
                Some('b') => format!("0b{digits}"),
                _ => digits,
            }
        } else {
            digits
        };
        let signed = match (negative, self.sign) {
            (true, _) => format!("-{prefixed}"),
            (false, Some('+')) => format!("+{prefixed}"),
            (false, _) => prefixed,
        };
        // 类型后缀紧跟数字, 与数字一并计入宽度.
        Ok(self.pad(format!("{signed}{suffix}")))
    }

    /// 渲染浮点数: 精度, 指数形态, 类型后缀与宽度填充.
    #[must_use]
    pub fn apply_float(&self, value: f64, suffix: &str) -> String {
        if !value.is_finite() {
            // NaN 与无穷不参与填充, 类型后缀仍然保留.
            return format!("{value}{suffix}");
        }
        let body = match (self.ty, self.precision) {
            (Some('e'), Some(p)) => format!("{value:.p$e}"),
            (Some('E'), Some(p)) => format!("{value:.p$E}"),
            (Some('e'), None) => format!("{value:e}"),
            (Some('E'), None) => format!("{value:E}"),
            (_, Some(p)) => format!("{value:.p$}"),
            // 只给宽度或对齐时仍用默认形态, `1.0` 不写成 `1`.
            _ => format_float_default(value),
        };
        self.finish_float(body, value == 0.0 && value.is_sign_negative(), suffix)
    }

    /// 渲染 f32: 精度与指数按 f32 求值, 默认形态取 f32 的最短往返表示.
    #[must_use]
    pub fn apply_float32(&self, value: f32, suffix: &str) -> String {
        if !value.is_finite() {
            return format!("{value}{suffix}");
        }
        let body = match (self.ty, self.precision) {
            (Some('e'), Some(p)) => format!("{value:.p$e}"),
            (Some('E'), Some(p)) => format!("{value:.p$E}"),
            (Some('e'), None) => format!("{value:e}"),
            (Some('E'), None) => format!("{value:E}"),
            (_, Some(p)) => format!("{value:.p$}"),
            _ => format_float32_default(value),
        };
        self.finish_float(body, value == 0.0 && value.is_sign_negative(), suffix)
    }

    /// 浮点数的符号与宽度处理; 负零的符号不能丢.
    fn finish_float(&self, body: String, negative_zero: bool, suffix: &str) -> String {
        let signed = if negative_zero && !body.starts_with('-') {
            format!("-{body}")
        } else if self.sign == Some('+') && !body.starts_with('-') {
            format!("+{body}")
        } else {
            body
        };
        self.pad(format!("{signed}{suffix}"))
    }

    /// 文本填充: 引号与多字节字符按字符数计宽.
    #[must_use]
    pub fn apply_text(&self, body: &str) -> String {
        self.pad_with(body.to_string(), false)
    }

    /// 数值填充: `0` 标志优先于 fill 与 align, 零填充紧贴符号与进制前缀.
    fn pad(&self, body: String) -> String {
        self.pad_with(body, true)
    }

    fn pad_with(&self, body: String, numeric: bool) -> String {
        let Some(width) = self.width else {
            return body;
        };
        let body_len = body.chars().count();
        if body_len >= width {
            return body;
        }
        let deficit = width - body_len;
        if self.zero {
            if numeric {
                // 零填充插在符号与进制前缀之后: `0x00ff`.
                let (sign, rest) = match body.as_bytes().first() {
                    Some(b'-' | b'+') => body.split_at(1),
                    _ => ("", body.as_str()),
                };
                let (prefix, rest) = split_int_prefix(rest);
                return format!("{sign}{prefix}{}{rest}", "0".repeat(deficit));
            }
            return format!("{}{body}", "0".repeat(deficit));
        }
        let fill = self.fill.unwrap_or(' ');
        match self.align.unwrap_or(Align::Right) {
            Align::Left => format!("{body}{}", fill.to_string().repeat(deficit)),
            Align::Right => format!("{}{body}", fill.to_string().repeat(deficit)),
            Align::Center => {
                let left = deficit / 2;
                let right = deficit - left;
                format!(
                    "{}{body}{}",
                    fill.to_string().repeat(left),
                    fill.to_string().repeat(right)
                )
            },
        }
    }
}

/// 拆分整数的进制前缀; 前缀不参与零填充的位数计算.
fn split_int_prefix(text: &str) -> (&str, &str) {
    if text.starts_with("0x") || text.starts_with("0b") {
        text.split_at(2)
    } else {
        ("", text)
    }
}

fn parse_align(c: char) -> Align {
    match c {
        '<' => Align::Left,
        '>' => Align::Right,
        _ => Align::Center,
    }
}

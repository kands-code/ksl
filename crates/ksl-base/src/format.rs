//! 格式化参数解析与数值渲染.
//!
//! 检查器与运行时共用同一套规则; 本层不接触运行时值.

/// 宽度上限; 只影响显示.
const MAX_WIDTH: usize = 256;
/// 精度上限; f64 的有效十进制位远少于此.
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
        FormatKind::Int => spec.precision.is_none() && !matches!(spec.ty, Some('e' | 'E')),
        FormatKind::Float => !spec.alternate && matches!(spec.ty, None | Some('e' | 'E')),
        FormatKind::Text => false,
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
    ty: Option<char>,
}

impl Spec {
    /// 解析 Rust 式格式参数子集: `[[fill]align][sign][#][0][width][.precision][type]`.
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
        if pos < chars.len() {
            spec.ty = Some(chars[pos]);
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

    /// 渲染浮点数: 精度, 指数形态与宽度填充.
    #[must_use]
    pub fn apply_float(&self, value: f64) -> String {
        if !value.is_finite() {
            // NaN 与无穷不参与填充, 原样输出.
            return value.to_string();
        }
        let body = match (self.ty, self.precision) {
            (Some('e'), Some(p)) => format!("{value:.p$e}"),
            (Some('E'), Some(p)) => format!("{value:.p$E}"),
            (Some('e'), None) => format!("{value:e}"),
            (Some('E'), None) => format!("{value:E}"),
            (_, Some(p)) => format!("{value:.p$}"),
            // 只给宽度或对齐时仍用默认形态, 例如 `1.0` 不能写成 `1`.
            _ => format_float_default(value),
        };
        let signed = if value.is_sign_negative() && value == 0.0 && !body.starts_with('-') {
            // 负零保持负号; 显式格式可能已经写出符号, 不能重复添加.
            format!("-{body}")
        } else if self.sign == Some('+') && !body.starts_with('-') {
            format!("+{body}")
        } else {
            body
        };
        self.pad(signed)
    }

    /// 宽度填充: `0` 标志优先于 fill 与 align, 零填充紧贴符号与进制前缀.
    fn pad(&self, body: String) -> String {
        let Some(width) = self.width else {
            return body;
        };
        if body.len() >= width {
            return body;
        }
        let deficit = width - body.len();
        if self.zero {
            // 零填充插在符号与进制前缀之后: `0x00ff` 而非 `00xff`.
            let (sign, rest) = match body.as_bytes().first() {
                Some(b'-' | b'+') => body.split_at(1),
                _ => ("", body.as_str()),
            };
            let (prefix, rest) = split_int_prefix(rest);
            return format!("{sign}{prefix}{}{rest}", "0".repeat(deficit));
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

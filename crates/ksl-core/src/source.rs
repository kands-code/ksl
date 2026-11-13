//! 源文件数据库与源码区间.
//!
//! [`Span`] 携带文件标识与字节区间, 行号与列号由 [`Sources`] 按需换算.
//! 位置信息同时服务 annotate-snippets 的终端诊断渲染与 LSP.

/// 源文件标识.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SourceId(u32);

/// 源码区间: 文件标识加字节偏移范围.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// 构造新区间, 供各词法例程在消费字符后标注记号范围.
    #[must_use]
    pub fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self { source, start, end }
    }
}

/// 源文件数据库; 持有全部源文本并预计算行起始偏移.
#[derive(Debug, Default)]
pub struct Sources {
    names: Vec<String>,
    texts: Vec<String>,
    line_starts: Vec<Vec<usize>>,
}

impl Sources {
    #[must_use]
    pub fn new() -> Self {
        Self {
            names: Vec::new(),
            texts: Vec::new(),
            line_starts: Vec::new(),
        }
    }

    /// 登记一份源文本, 返回其标识.
    ///
    /// # Panics
    ///
    /// 登记的源文件数量超出 `u32` 上限时 panic; 该上限在实际使用中不可能达到.
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        let text = text.into();
        let line_starts = Self::compute_line_starts(&text);
        let id = SourceId(u32::try_from(self.texts.len()).expect("源文件数量超出 u32 上限"));
        self.names.push(name.into());
        self.texts.push(text);
        self.line_starts.push(line_starts);
        id
    }

    /// 每个换行符 `'\n'` 之后的位置视为新行起点, 首行从 0 开始.
    fn compute_line_starts(text: &str) -> Vec<usize> {
        let mut starts = vec![0];
        starts.extend(
            text.bytes().enumerate().filter_map(
                |(i, b)| {
                    if b == b'\n' { Some(i + 1) } else { None }
                },
            ),
        );
        starts
    }

    #[must_use]
    pub fn name(&self, id: SourceId) -> &str {
        &self.names[id.0 as usize]
    }

    #[must_use]
    pub fn text(&self, id: SourceId) -> &str {
        &self.texts[id.0 as usize]
    }

    /// 字节偏移所在的行号, 从 0 开始; 渲染展示时由调用方换算为 1 起始.
    #[must_use]
    pub fn line_index(&self, id: SourceId, offset: usize) -> usize {
        self.line_starts[id.0 as usize]
            .partition_point(|&start| start <= offset)
            .saturating_sub(1)
    }

    /// 字节偏移所在的行与列, 均从 0 开始, 列按字节计.
    ///
    /// 终端渲染由 annotate-snippets 自行换算列宽; LSP 使用
    /// [`Self::line_col_utf16`] 与 [`Self::offset_at_utf16`].
    #[must_use]
    pub fn line_col(&self, id: SourceId, offset: usize) -> (usize, usize) {
        let line = self.line_index(id, offset);
        let col = offset - self.line_starts[id.0 as usize][line];
        (line, col)
    }

    /// 字节偏移所在的行与 UTF-16 列, 均从 0 开始.
    ///
    /// LSP 的默认位置编码是 UTF-16 code unit, 与字节列不同.
    #[must_use]
    pub fn line_col_utf16(&self, id: SourceId, offset: usize) -> (usize, usize) {
        let text = self.text(id);
        let mut offset = offset.min(text.len());
        while offset > 0 && !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let line = self.line_index(id, offset);
        let start = self.line_starts[id.0 as usize][line];
        let character = text[start..offset].chars().map(char::len_utf16).sum();
        (line, character)
    }

    /// 把 UTF-16 行列换算为字节偏移; 超出范围时钳制到行尾.
    ///
    /// LSP 位置可能落在代理对中间或越过行尾, 这里统一落到最近的
    /// 字符边界, 不 panic.
    #[must_use]
    pub fn offset_at_utf16(&self, id: SourceId, line: usize, character: usize) -> usize {
        let text = self.text(id);
        let Some(&start) = self.line_starts[id.0 as usize].get(line) else {
            return text.len();
        };
        let end = self.line_starts[id.0 as usize]
            .get(line + 1)
            .copied()
            .unwrap_or(text.len());
        let line_text = &text[start..end];
        let mut units = 0;
        for (byte, ch) in line_text.char_indices() {
            if units >= character {
                return start + byte;
            }
            units += ch.len_utf16();
        }
        start + line_text.trim_end_matches(['\n', '\r']).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_positions_round_trip() {
        let mut sources = Sources::default();
        let id = sources.add("<test>", "aé😀\nxy");
        assert_eq!(sources.line_col_utf16(id, 1), (0, 1));
        assert_eq!(sources.line_col_utf16(id, 3), (0, 2));
        assert_eq!(sources.line_col_utf16(id, 7), (0, 4));
        assert_eq!(sources.line_col_utf16(id, 8), (1, 0));
        assert_eq!(sources.offset_at_utf16(id, 0, 0), 0);
        assert_eq!(sources.offset_at_utf16(id, 0, 1), 1);
        assert_eq!(sources.offset_at_utf16(id, 0, 2), 3);
        assert_eq!(sources.offset_at_utf16(id, 0, 4), 7);
        assert_eq!(sources.offset_at_utf16(id, 1, 2), 10);
        assert_eq!(sources.offset_at_utf16(id, 1, 99), 10);
    }
}

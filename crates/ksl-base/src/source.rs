//! 源文件数据库与源码区间.
//!
//! [`Span`] 携带文件标识与字节区间, 行号与列号由 [`Sources`] 按需换算.
//! 位置信息同时服务 annotate-snippets 的终端诊断渲染与 LSP.

use line_index::{LineCol, LineIndex, TextSize, WideEncoding, WideLineCol};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SourceId(u32);

/// 源码区间: 文件标识加字节偏移范围.
///
/// 偏移以 `u32` 保存; 单份源码达到 4 GiB 时 panic, 与行索引的假设一致.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub source: SourceId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// 以字节偏移构造区间.
    ///
    /// # Panics
    ///
    /// 偏移超出 `u32` 上限时 panic; 单份源码以 4 GiB 为上限.
    #[must_use]
    pub fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self {
            source,
            start: u32::try_from(start).expect("源码偏移超出 u32 上限"),
            end: u32::try_from(end).expect("源码偏移超出 u32 上限"),
        }
    }

    /// 起点的字节偏移.
    #[must_use]
    pub fn start(self) -> usize {
        self.start as usize
    }

    /// 终点的字节偏移.
    #[must_use]
    pub fn end(self) -> usize {
        self.end as usize
    }
}

/// 源文件数据库; 持有全部源文本与行索引.
#[derive(Debug, Default)]
pub struct Sources {
    names: Vec<String>,
    texts: Vec<String>,
    line_indexes: Vec<LineIndex>,
}

impl Sources {
    #[must_use]
    pub fn new() -> Self {
        Self {
            names: Vec::new(),
            texts: Vec::new(),
            line_indexes: Vec::new(),
        }
    }

    /// 登记一份源文本, 返回其标识.
    ///
    /// # Panics
    ///
    /// 登记的源文件数量超出 `u32` 上限, 或单份源码达到 4 GiB 时 panic.
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        let text = text.into();
        let line_index = LineIndex::new(&text);
        let id = SourceId(u32::try_from(self.texts.len()).expect("源文件数量超出 u32 上限"));
        self.names.push(name.into());
        self.texts.push(text);
        self.line_indexes.push(line_index);
        id
    }

    #[must_use]
    pub fn name(&self, id: SourceId) -> &str {
        &self.names[id.0 as usize]
    }

    #[must_use]
    pub fn text(&self, id: SourceId) -> &str {
        &self.texts[id.0 as usize]
    }

    /// 字节偏移所在的行与列, 均从 0 开始, 列按字节计.
    ///
    /// 终端渲染由 annotate-snippets 自行换算列宽.
    /// LSP 使用 [`Self::line_col_utf16`] 与 [`Self::offset_at_utf16`].
    #[must_use]
    pub fn line_col(&self, id: SourceId, offset: usize) -> (usize, usize) {
        let text = self.text(id);
        let offset = floor_char_boundary(text, offset);
        let line_col = self.line_indexes[id.0 as usize]
            .try_line_col(text_size(offset))
            .unwrap_or(LineCol { line: 0, col: 0 });
        (line_col.line as usize, line_col.col as usize)
    }

    /// 字节偏移所在的行与 UTF-16 列, 均从 0 开始.
    ///
    /// LSP 的默认位置编码是 UTF-16 code unit, 与字节列不同.
    #[must_use]
    pub fn line_col_utf16(&self, id: SourceId, offset: usize) -> (usize, usize) {
        let (line, col) = self.line_col(id, offset);
        let line_col = LineCol {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            col: u32::try_from(col).unwrap_or(u32::MAX),
        };
        self.line_indexes[id.0 as usize]
            .to_wide(WideEncoding::Utf16, line_col)
            .map_or((line, 0), |wide| (wide.line as usize, wide.col as usize))
    }

    /// 把 UTF-16 行列换算为字节偏移; 超出范围时钳制到行尾.
    ///
    /// LSP 位置可能落在代理对中间或越过行尾.
    /// 落在代理对中间时取其后的字符边界, 越过行尾时钳制到行尾, 均不 panic.
    #[must_use]
    pub fn offset_at_utf16(&self, id: SourceId, line: usize, character: usize) -> usize {
        let text = self.text(id);
        let index = &self.line_indexes[id.0 as usize];
        let line = u32::try_from(line).unwrap_or(u32::MAX);
        let Some(range) = index.line(line) else {
            return text.len();
        };
        let start = usize::try_from(u32::from(range.start())).unwrap_or(usize::MAX);
        let end = usize::try_from(u32::from(range.end())).unwrap_or(usize::MAX);
        let line_end = start + text[start..end].trim_end_matches(['\n', '\r']).len();
        let wide = WideLineCol {
            line,
            col: u32::try_from(character).unwrap_or(u32::MAX),
        };
        let Some(utf8) = index.to_utf8(WideEncoding::Utf16, wide) else {
            return line_end;
        };
        let Some(offset) = index.offset(utf8) else {
            return line_end;
        };
        let mut offset = usize::try_from(u32::from(offset)).unwrap_or(usize::MAX);
        if offset > line_end {
            return line_end;
        }
        while offset < line_end && !text.is_char_boundary(offset) {
            offset += 1;
        }
        offset
    }
}

/// 把字节偏移降到字符边界, 并钳制到源码长度.
fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// 把字节偏移转为 `TextSize`; 超过 `u32` 的源码已在 `add` 处排除.
fn text_size(offset: usize) -> TextSize {
    TextSize::from(u32::try_from(offset).unwrap_or(u32::MAX))
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

    #[test]
    fn utf16_offsets_clamp_to_valid_boundaries() {
        // 代理对中点向前进到字符边界.
        let mut sources = Sources::default();
        let id = sources.add("<test>", "a😀b");
        assert_eq!(sources.offset_at_utf16(id, 0, 2), 5);
        assert_eq!(sources.line_col_utf16(id, 5), (0, 3));

        // CRLF 的列偏移不进入换行符, 行尾偏移钳制到行末.
        let mut sources = Sources::default();
        let id = sources.add("<test>", "a\r\nb");
        assert_eq!(sources.line_col_utf16(id, 1), (0, 1));
        assert_eq!(sources.line_col_utf16(id, 3), (1, 0));
        assert_eq!(sources.offset_at_utf16(id, 0, 99), 1);
        assert_eq!(sources.offset_at_utf16(id, 1, 99), 4);
    }
}

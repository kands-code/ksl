//! 内存历史; 整段输入一条, 不落文件.

pub(super) struct History {
    entries: Vec<String>,
    /// 当前浏览位置; None 表示正在编辑新输入.
    index: Option<usize>,
    /// 开始浏览历史前的草稿.
    draft: String,
}

impl History {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            draft: String::new(),
        }
    }

    pub(super) fn add(&mut self, entry: String) {
        if entry.is_empty() {
            return;
        }
        self.entries.push(entry);
        self.reset();
    }

    pub(super) fn reset(&mut self) {
        self.index = None;
        self.draft.clear();
    }

    /// 上一条历史; `current` 是开始浏览前的当前行.
    pub(super) fn previous(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        match self.index {
            None => {
                self.draft = current.to_string();
                self.index = Some(self.entries.len() - 1);
            },
            Some(0) => {},
            Some(index) => self.index = Some(index - 1),
        }
        self.index.map(|index| self.entries[index].clone())
    }

    /// 下一条历史; 越过末尾时回到草稿.
    pub(super) fn next(&mut self) -> Option<String> {
        match self.index {
            None => None,
            Some(index) if index + 1 < self.entries.len() => {
                self.index = Some(index + 1);
                Some(self.entries[index + 1].clone())
            },
            Some(_) => {
                self.index = None;
                Some(self.draft.clone())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigates_entries_and_draft() {
        let mut history = History::new();
        history.add("one".to_string());
        history.add("two".to_string());

        assert_eq!(history.previous("draft").as_deref(), Some("two"));
        assert_eq!(history.previous("draft").as_deref(), Some("one"));
        assert_eq!(history.previous("draft").as_deref(), Some("one"));
        assert_eq!(history.next().as_deref(), Some("two"));
        assert_eq!(history.next().as_deref(), Some("draft"));
        assert_eq!(history.next(), None);
    }
}

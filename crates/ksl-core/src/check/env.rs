//! 检查环境: 作用域栈.
//!
//! 与求值模型一致: `@Let` 压入新绑定并可遮蔽外层, `@Block` 对应一个新
//! 作用域层. 绑定不可变, 故作用域只记类型, 无需记录可变性.

use super::types::Type;
use crate::symbol::Symbol;

/// 一层作用域: 绑定表.
///
/// 持久化映射让会话克隆只复制结构头; 长会话的全局绑定不会再每轮输入
/// 被整体深拷贝.
#[derive(Debug, Clone, Default)]
struct Scope {
    map: imbl::HashMap<Symbol, Type>,
}

/// 作用域栈; 查找从内向外, 插入只影响当前层.
/// 默认含一个全局层.
#[derive(Debug, Clone)]
pub struct Env {
    scopes: Vec<Scope>,
}

impl Default for Env {
    fn default() -> Self {
        Self {
            scopes: vec![Scope::default()],
        }
    }
}

impl Env {
    /// 进入新作用域.
    pub fn push(&mut self) {
        self.scopes.push(Scope {
            map: imbl::HashMap::new(),
        });
    }

    /// 离开当前作用域层, 其中绑定随之失效.
    pub fn pop(&mut self) {
        self.scopes.pop();
    }

    /// 在当前层登记绑定; 遮蔽外层同名绑定.
    pub fn insert(&mut self, name: Symbol, ty: Type) {
        self.scopes
            .last_mut()
            .expect("作用域栈至少保留全局层")
            .map
            .insert(name, ty);
    }

    /// 从内向外查找绑定的类型.
    pub fn lookup(&self, name: Symbol) -> Option<&Type> {
        self.scopes.iter().rev().find_map(|scope| scope.map.get(&name))
    }

    /// 当前会话的全局绑定快照; REPL 展示用.
    #[must_use]
    pub fn global_bindings(&self) -> Vec<(Symbol, Type)> {
        let Some(scope) = self.scopes.first() else {
            return Vec::new();
        };
        scope.map.iter().map(|(name, ty)| (*name, ty.clone())).collect()
    }
}

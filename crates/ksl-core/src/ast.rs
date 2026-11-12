//! 抽象语法树.
//!
//! 节点按语言结构分家族定义, 特殊形式各有专属变体, 便于类型检查阶段
//! 穷尽处理; 每种节点携带源码区间, 供诊断渲染与 LSP 使用.

use crate::source::{SourceId, Span};
use crate::symbol::Symbol;

mod ops;
pub use ops::{
    dict_op_kind, list_op_kind, operation_names, option_op_kind, pair_op_kind,
    registry_op_kind, set_op_kind,
};

/// 源文件内的节点身份; 由解析器按顺序分配.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    /// 由解析器分配的编号构造.
    #[must_use]
    pub fn from_index(index: u32) -> Self {
        Self(index)
    }

    /// 内部编号; 供调试与索引使用.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

/// 跨源唯一的节点键; 类型表以它为主键.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeKey {
    pub source: SourceId,
    pub node: NodeId,
}

impl NodeKey {
    #[must_use]
    pub fn new(source: SourceId, node: NodeId) -> Self {
        Self { source, node }
    }
}

/// 带区间与节点身份的语法节点包装.
///
/// 节点身份只用于类型表等按节点查询的场景; 结构比较忽略它, 避免同一段
/// 源码重复解析后因编号不同而不相等.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
    pub id: NodeId,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span, id: NodeId) -> Self {
        Self { node, span, id }
    }

    /// 节点在类型表中的键.
    #[must_use]
    pub fn key(&self) -> NodeKey {
        NodeKey::new(self.span.source, self.id)
    }
}

impl<T: PartialEq> PartialEq for Spanned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node && self.span == other.span
    }
}

/// 类型表达式; 出现在绑定标注, `@Fn`/`@Def`/`@Handler` 签名与 `@Data` 字段等处.
#[derive(Debug, Clone)]
pub enum TypeExpr {
    /// 具名类型: 内置类型或 Data 类型名, 如 `i64`, `Shape`.
    Named(Spanned<Symbol>),
    /// 内置参数化容器: `List[a]`, `Dict[K, V]`, `Set[T]`, `Option[T]`,
    /// `Result[T, E]`, `Pair[K, V]`, `Pid[T0, ..., Tn-1, R]`, `Handler[...]`.
    Generic {
        name: Spanned<Symbol>,
        args: Vec<TypeExpr>,
    },
    /// 函数类型: `Fn[[参数, ...] : 返回]`, 与 `@Fn` 形态同构.
    Fn {
        /// 类型表达式自身的节点身份.
        id: NodeId,
        span: crate::source::Span,
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
}

impl TypeExpr {
    /// 类型表达式在类型表中的键.
    #[must_use]
    pub fn key(&self) -> NodeKey {
        match self {
            Self::Named(name) | Self::Generic { name, .. } => name.key(),
            Self::Fn { id, span, .. } => NodeKey::new(span.source, *id),
        }
    }
}

impl PartialEq for TypeExpr {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Named(left), Self::Named(right)) => left == right,
            (
                Self::Generic {
                    name: left_name,
                    args: left_args,
                },
                Self::Generic {
                    name: right_name,
                    args: right_args,
                },
            ) => left_name == right_name && left_args == right_args,
            (
                Self::Fn {
                    span: left_span,
                    params: left_params,
                    ret: left_ret,
                    ..
                },
                Self::Fn {
                    span: right_span,
                    params: right_params,
                    ret: right_ret,
                    ..
                },
            ) => {
                left_span == right_span && left_params == right_params && left_ret == right_ret
            },
            _ => false,
        }
    }
}

/// 调用头部: 普通名字或受限名字 `Type/Constructor`.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub qualifier: Option<Spanned<Symbol>>,
    pub name: Spanned<Symbol>,
}

/// 格式化字符串的插值片段; 表达式在语法阶段完成变量与字面量校验.
#[derive(Debug, Clone, PartialEq)]
pub enum FStrAstPart {
    Text(String),
    Interp {
        expr: Box<Expr>,
        spec: Option<String>,
    },
}

/// 表达式节点.
#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    I64(i64),
    U8(u8),
    U64(u64),
    F64(f64),
    Char(char),
    Str(String),
    FStr(Vec<FStrAstPart>),
    Bool(bool),
    /// nil 字面值.
    Nil,
    Ident(Symbol),
    /// 列表字面量 `{a, b, c}`.
    List(Vec<Expr>),
    /// 配对记号 `(a, b)`.
    Pair(Box<Expr>, Box<Expr>),
    /// 普通调用 `f[x, y]`.
    Call {
        head: Head,
        args: Vec<Expr>,
    },
    Special(Special),
}

/// 带区间的表达式, 供检查错误定位.
pub type Expr = Spanned<ExprKind>;

/// 特殊形式节点家族.
#[derive(Debug, Clone, PartialEq)]
pub enum Special {
    /// `@Let[绑定目标, 值]`.
    Let {
        target: BindingTarget,
        value: Box<Expr>,
    },
    /// `@Block[语句, ...]`, 值为最后一条语句.
    Block(Vec<Expr>),
    /// `@If[cond, then, else]`.
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    /// 匿名函数 `@Fn[[x : i64] : i64, 体]`; 值即函数.
    Fn {
        params: Vec<Param>,
        ret: TypeExpr,
        body: Box<Expr>,
    },
    /// 具名函数定义 `@Def[名字, [x : i64] : i64, 体]`; 语句, 值 nil.
    Def(Def),
    /// 顶层退出 `@Exit[code]`; 语句, 类型 Never, 只能出现在顶层.
    Exit {
        code: Box<Expr>,
    },
    /// Actor 蓝图 `@Handler[[self : Pid[...], 状态...] : R, 体]`.
    Handler {
        params: Vec<Param>,
        ret: TypeExpr,
        body: Box<Expr>,
    },
    /// Handler 自循环 `@Continue[新状态...]`; 类型 Never, 仅限 Handler 体内.
    Continue {
        args: Vec<Expr>,
    },
    /// 启动 Actor: `@Spawn[N, handler, 实参...]` 或 `@SpawnDetached[...]`.
    Spawn {
        count: Box<Expr>,
        handler: Box<Expr>,
        args: Vec<Expr>,
        detached: bool,
    },
    /// `@Send[pid, 索引, 消息]`.
    Send {
        pid: Box<Expr>,
        index: Box<Expr>,
        message: Box<Expr>,
    },
    /// `@Receive[索引, 超时]`, 仅限 Handler 体内.
    Receive {
        index: Box<Expr>,
        timeout: Box<Expr>,
    },
    /// `@Join[pid]`.
    Join {
        pid: Box<Expr>,
    },
    /// `@Kill[pid]`.
    Kill {
        pid: Box<Expr>,
    },
    /// `@Monitor[pid]`, 目标终止时向本 Actor 的最后一个信箱投递监视通知.
    Monitor {
        pid: Box<Expr>,
    },
    /// `@Unmonitor[监视编号]`.
    Unmonitor {
        id: Box<Expr>,
    },
    /// `@PidSerial[pid]`, 取调度器内的 Actor 编号.
    PidSerial {
        pid: Box<Expr>,
    },
    /// `@SetTimer[pid, 索引, 消息, 毫秒]`, 延迟投递消息.
    SetTimer {
        pid: Box<Expr>,
        index: Box<Expr>,
        message: Box<Expr>,
        millis: Box<Expr>,
    },
    /// `@CancelTimer[定时编号]`.
    CancelTimer {
        id: Box<Expr>,
    },
    /// `@Halt[]`, 仅限 Handler 体内.
    Halt,
    /// `@Sleep[毫秒]`, 仅限 Handler 体内.
    Sleep {
        millis: Box<Expr>,
    },
    /// `@Fix[定义, ...]`, 仅接受 `@Def`.
    Fix(Vec<Def>),
    /// `@Data[Name, {Variant[字段, ...], ...}]`, 仅允许出现在顶层.
    Data {
        name: Spanned<Symbol>,
        variants: Vec<DataVariant>,
    },
    /// `@Export[名字, ..., ^类型, ...]`, 仅允许出现在顶层.
    Export {
        items: Vec<ExportItem>,
    },
    /// `@Use[{名字, (旧名, 新名), ^类型, ^(旧名, 新名)}, "路径"]`.
    Use {
        items: Vec<UseItem>,
        path: String,
    },
    /// `@Match[值, {(模式, 结果), ...}]`.
    Match {
        value: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// 列表操作族, 形态与类型规则各异, 由类型检查阶段逐项校验.
    ListOp {
        op: ListOpKind,
        args: Vec<Expr>,
    },
    Dict {
        entries: Vec<Expr>,
    },
    Set {
        items: Vec<Expr>,
    },
    PairOp {
        op: PairOpKind,
        args: Vec<Expr>,
    },
    DictOp {
        op: DictOpKind,
        args: Vec<Expr>,
    },
    SetOp {
        op: SetOpKind,
        args: Vec<Expr>,
    },
    Registry {
        op: RegistryOp,
        args: Vec<Expr>,
    },
    /// `@IsSome[opt]` 等 Option 与 Result 的便捷操作.
    OptionOp {
        op: OptionOpKind,
        args: Vec<Expr>,
    },
    /// `@Assert[条件, 信息]`: 条件为假则断言失败, 为真则继续.
    Assert {
        cond: Box<Expr>,
        message: Box<Expr>,
    },
    /// `@Panic[信息]`: 主动中止.
    Panic {
        message: Box<Expr>,
    },
    /// `@Todo[信息]`: 尚未实现.
    Todo {
        message: Box<Expr>,
    },
    /// `@Unreachable[信息]`: 不该到达的分支.
    Unreachable {
        message: Box<Expr>,
    },
}

/// 用户主动中断的类别; 四个原语共用一条失败通道, 但错误条目各自独立.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    /// 断言失败.
    Assertion,
    /// 主动中止.
    Panic,
    /// 尚未实现.
    Unimplemented,
    /// 不该到达的分支.
    Unreachable,
}

impl std::fmt::Display for FailKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Assertion => "assert",
            Self::Panic => "panic",
            Self::Unimplemented => "todo",
            Self::Unreachable => "unreachable",
        };
        f.write_str(text)
    }
}

/// 列表操作种类.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOpKind {
    Get,
    Update,
    Prepend,
    Append,
    Concat,
    Head,
    Tail,
    Take,
    Drop,
    Reverse,
    Repeat,
    Flatten,
    Chunk,
    TakeWhile,
    DropWhile,
    Map,
    MapIndexed,
    Filter,
    Fold,
    PrefixFold,
    Any,
    All,
    Length,
    Riffle,
    Window,
    Zip,
    ZipWith,
}

/// 配对访问操作种类.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairOpKind {
    Fst,
    Snd,
}

/// 命名注册表操作种类.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryOp {
    Register,
    WhereIs,
    Unregister,
}

/// Dict 操作种类.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictOpKind {
    Lookup,
    Get,
    Put,
    Update,
    Remove,
    Has,
    Length,
    Entries,
    Keys,
    Values,
    Merge,
    Filter,
    FromEntries,
}

/// Set 操作种类.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOpKind {
    Insert,
    Remove,
    Has,
    Length,
    Items,
    Union,
    Intersect,
    Difference,
    Filter,
    FromList,
}

/// Option 与 Result 的便捷操作种类.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionOpKind {
    /// 判定是否为 Some, 结果为 bool.
    IsSome,
    /// 判定是否为 None, 结果为 bool.
    IsNone,
    /// 判定是否为 Ok, 结果为 bool.
    IsOk,
    /// 判定是否为 Err, 结果为 bool.
    IsErr,
    /// Some 时应用函数并重新包装, None 时原样返回.
    OptionMap,
    /// Ok 时应用函数并重新包装, Err 时原样返回.
    ResultMap,
    /// Some 时应用函数, None 时原样返回.
    OptionAndThen,
    /// Ok 时应用函数, Err 时原样返回.
    ResultAndThen,
    /// Some 取值, None 取默认值.
    OptionGet,
    /// Ok 取值, Err 取默认值.
    ResultGet,
}

/// 绑定目标: `名字` 或 `名字 : 类型`.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingTarget {
    /// `_` 表示显式抛弃.
    pub name: Option<Spanned<Symbol>>,
    pub ty: Option<TypeExpr>,
}

/// 具名函数定义: `@Def[名字, [参数...] : 返回类型, 体]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    pub name: Spanned<Symbol>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Box<Expr>,
}

/// 函数形参: `x : i64`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Spanned<Symbol>,
    pub ty: TypeExpr,
}

/// `@Data` 变体: `Circle[f64, f64]`.
#[derive(Debug, Clone, PartialEq)]
pub struct DataVariant {
    pub name: Spanned<Symbol>,
    pub fields: Vec<TypeExpr>,
}

/// `@Export` 条目: 值或类型.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportItem {
    Value(Spanned<Symbol>),
    Type(Spanned<Symbol>),
}

/// `@Use` 条目: 导入或重命名.
#[derive(Debug, Clone, PartialEq)]
pub enum UseItem {
    Value(Spanned<Symbol>),
    Rename {
        from: Spanned<Symbol>,
        to: Spanned<Symbol>,
    },
    Type(Spanned<Symbol>),
    TypeRename {
        from: Spanned<Symbol>,
        to: Spanned<Symbol>,
    },
}

/// `@Match` 分支: `(模式, 结果)`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

/// 构造器模式: `[类型/]构造器[字段绑定, ...]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// 省略类型时由被匹配值的类型推断.
    pub qualifier: Option<Spanned<Symbol>>,
    /// 变体名; None 为通配模式, 匹配任意变体且不绑定字段.
    pub variant: Option<Spanned<Symbol>>,
    /// 字段绑定为变量名或 `_`, 仅用于 `@Match`.
    pub fields: Vec<PatternField>,
}

/// 模式字段: 变量绑定或通配.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternField {
    Bind(Spanned<Symbol>),
    Wildcard,
}

/// 顶层语句; 语句即表达式, 以 `;` 结尾.
pub type Stmt = Expr;

/// 顶层程序: 以分号结尾的语句序列加可选的尾表达式.
/// 尾表达式省略末尾分号, 其值即程序值; REPL 直接展示, `run` 静默丢弃.
#[derive(Debug, Clone)]
pub struct Program {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Expr>,
}

impl Program {
    /// 全部顶层表达式: 语句在前, 尾表达式最后.
    pub fn exprs(&self) -> impl Iterator<Item = &Expr> {
        self.stmts.iter().chain(self.tail.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Sources;

    /// 节点身份不参与结构比较, 但必须构成唯一键.
    #[test]
    fn node_identity_is_separate_from_structure() {
        let mut sources = Sources::default();
        let first_source = sources.add("a", "1");
        let second_source = sources.add("b", "1");

        let first = Spanned::new(1, Span::new(first_source, 0, 1), NodeId::from_index(0));
        let second = Spanned::new(1, Span::new(first_source, 0, 1), NodeId::from_index(1));
        let other_source =
            Spanned::new(1, Span::new(second_source, 0, 1), NodeId::from_index(0));

        assert_eq!(first, second);
        assert_ne!(first.key(), second.key());
        assert_ne!(first.key(), other_source.key());
    }
}

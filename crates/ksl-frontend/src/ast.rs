//! 抽象语法树.
//!
//! 节点按语言结构分家族定义, 特殊形式各有专属变体.
//! 每种节点携带源码区间, 供诊断渲染与 LSP 使用.

use ksl_base::name::Name;
use ksl_base::source::Span;
pub use ksl_catalog::forms::{FormKind, special_form_names};
pub use ksl_catalog::names::{HighlightGroup, highlight_group};
pub use ksl_catalog::ops::{
    DictOpKind, ListOpKind, OptionOpKind, PairOpKind, SetOpKind, dict_op_kind, list_op_kind,
    operation_names, option_op_kind, pair_op_kind, set_op_kind,
};

/// 带源码区间的语法节点包装.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

/// 类型表达式; 出现在绑定标注, `@Fn`/`@Def`/`@Handler` 签名与 `@Data` 字段等处.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// 具名类型: 内置类型或 Data 类型名, 如 `i64`, `Shape`.
    Named(Spanned<Name>),
    /// 内置参数化容器: `List[T]`, `Dict[K, V]`, `Set[T]`, `Option[T]`,
    /// `Result[T, E]`, `Pair[A, B]`, `Pid[T0, ..., Tn-1, R]`, `Handler[...]`.
    Generic {
        name: Spanned<Name>,
        args: Vec<TypeExpr>,
    },
    /// 函数类型: `Fn[[参数, ...] : 返回]`, 与 `@Fn` 形态同构.
    Fn {
        span: ksl_base::source::Span,
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
}

/// 调用头部: 普通名字或类型限定名 `Type/Constructor`.
#[derive(Debug, Clone, PartialEq)]
pub struct Head {
    pub qualifier: Option<Spanned<Name>>,
    pub name: Spanned<Name>,
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
    I32(i32),
    U8(u8),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Char(char),
    Str(String),
    FStr(Vec<FStrAstPart>),
    Bool(bool),
    /// nil 字面值.
    Nil,
    Ident(Name),
    /// 列表字面量 `{a, b, c}`.
    List(Vec<Expr>),
    /// 配对记号 `(a, b)`.
    Pair(Box<Expr>, Box<Expr>),
    /// 普通调用 `f[x, y]`.
    Call {
        head: Head,
        args: Vec<Expr>,
    },
    /// 后缀调用: 对任意表达式结果的应用 `expr[x, y]`, 可链式.
    ///
    /// 每对方括号是一次完整调用, 不做柯里化; 调用目标须为函数类型.
    Apply {
        callee: Box<Expr>,
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
    /// `@Which[{(cond, value), ...}, fallback]`.
    Which {
        branches: Vec<(Expr, Expr)>,
        fallback: Box<Expr>,
    },
    /// 匿名函数 `@Fn[[x : i64] : i64, 体]`; 值即函数.
    Fn {
        params: Vec<Param>,
        ret: TypeExpr,
        body: Box<Expr>,
    },
    /// 具名函数定义 `@Def[名字, [x : i64] : i64, 体]`; 语句, 值 nil.
    Def(Def),
    /// 退出 `@Exit[code]`; 类型 Never, 只能出现在程序根或 `@Main` 体内.
    Exit {
        code: Box<Expr>,
    },
    /// Actor 蓝图 `@Handler[[self : Pid[...], 状态...] : R, 体]`.
    Handler {
        params: Vec<Param>,
        ret: TypeExpr,
        body: Box<Expr>,
    },
    /// 模块入口 `@Main[[self : Pid[...]] : nil, 体]`; 程序根 Actor.
    Main {
        params: Vec<Param>,
        ret: TypeExpr,
        body: Box<Expr>,
    },
    /// Handler 自循环 `@Continue[新状态...]`; 类型 Never, 仅限 Handler 体内.
    Continue {
        args: Vec<Expr>,
    },
    /// 启动 Actor: `@Spawn[handler, 实参...]` 或 `@SpawnDetached[...]`.
    ///
    /// 信箱数由 Handler 的 Pid 类型推断, 不在调用点重复书写.
    Spawn {
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
    /// `@DataGet[值, 下标]`, 读取单变体 Data 的字段.
    DataGet {
        value: Box<Expr>,
        index: Box<Expr>,
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
        name: Spanned<Name>,
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
    /// `@Link["插件名", {^类型, ...}, {名字 : 类型, ...}]`.
    Link(LinkDecl),
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

/// 绑定目标: `名字` 或 `名字 : 类型`.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingTarget {
    /// `_` 表示显式丢弃.
    pub name: Option<Spanned<Name>>,
    pub ty: Option<TypeExpr>,
}

/// 具名函数定义: `@Def[名字, [参数...] : 返回类型, 体]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Def {
    pub name: Spanned<Name>,
    pub params: Vec<Param>,
    pub ret: TypeExpr,
    pub body: Box<Expr>,
}

/// 函数形参: `x : i64`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Spanned<Name>,
    pub ty: TypeExpr,
}

/// `@Data` 变体: `Circle[f64, f64]`.
#[derive(Debug, Clone, PartialEq)]
pub struct DataVariant {
    pub name: Spanned<Name>,
    pub fields: Vec<TypeExpr>,
}

/// `@Export` 条目: 值或类型.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportItem {
    Value(Spanned<Name>),
    Type(Spanned<Name>),
}

/// `@Use` 条目: 导入或重命名.
#[derive(Debug, Clone, PartialEq)]
pub enum UseItem {
    Value(Spanned<Name>),
    Rename {
        from: Spanned<Name>,
        to: Spanned<Name>,
    },
    Type(Spanned<Name>),
    TypeRename {
        from: Spanned<Name>,
        to: Spanned<Name>,
    },
}

/// `@Link` 声明: 插件名, 不透明类型与值.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkDecl {
    pub plugin: String,
    pub types: Vec<Spanned<Name>>,
    pub values: Vec<LinkValue>,
}

/// 插件中的一个值声明.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkValue {
    pub name: Spanned<Name>,
    pub ty: TypeExpr,
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
    pub qualifier: Option<Spanned<Name>>,
    /// 变体名; None 为通配模式, 匹配任意变体且不绑定字段.
    pub variant: Option<Spanned<Name>>,
    /// 字段绑定为变量名或 `_`, 仅用于 `@Match`.
    pub fields: Vec<PatternField>,
}

/// 模式字段: 变量绑定或通配.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternField {
    Bind(Spanned<Name>),
    Wildcard,
}

/// 顶层语句; 语句即表达式, 以 `;` 结尾.
pub type Stmt = Expr;

/// 顶层程序: 以分号结尾的语句序列加可选的尾表达式.
/// 尾表达式省略末尾分号, 其值即程序值. REPL 直接展示, `run` 静默丢弃.
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

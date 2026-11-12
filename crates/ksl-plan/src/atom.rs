//! CPS IR 与线性 IR 共有的原子结构与格式化字符串片段.
//!
//! 两套 IR 的字面量与复合值形态一致, 仅变量引用不同.
//! 变量相关的语义由各自 IR 实现.
//! 复合值装箱, 使原子以变量与标量为主时保持紧凑.

use ksl_base::name::Name;
use ksl_base::types::Type;

/// 列表字面量; `elem` 为元素类型.
#[derive(Debug, Clone)]
pub struct ListAtom<V> {
    pub elem: Type,
    pub items: Vec<V>,
}

/// Data 构造; 类型与变体名均为真实身份.
#[derive(Debug, Clone)]
pub struct DataAtom<V> {
    pub ty: Name,
    pub variant: Name,
    pub fields: Vec<V>,
}

/// 模块内函数引用; 捕获在创建点求值.
#[derive(Debug, Clone)]
pub struct FuncAtom<V> {
    pub name: Name,
    pub captures: Vec<V>,
}

/// 格式化字符串片段; 插值值由使用方的 IR 原子类型填充.
#[derive(Debug, Clone)]
pub enum FStrPart<V> {
    Text(String),
    Interp { value: V, spec: Option<String> },
}

/// 定义一套 IR 原子: 共有字面量与复合值, 加上调用方给出的变量变体.
#[macro_export]
macro_rules! define_atom {
    ($name:ident, $fstr:ty, $($extra:tt)*) => {
        /// IR 原子值; 复合值的子项均已求值为原子.
        #[derive(Debug, Clone)]
        pub enum $name {
            I64(i64),
            I32(i32),
            U8(u8),
            U32(u32),
            U64(u64),
            F32(f32),
            F64(f64),
            Char(char),
            Str(ArcStr),
            Bool(bool),
            Nil,
            /// 内置函数值; 编号在降级期解析.
            Builtin(BuiltinId),
            /// 列表字面量.
            List(Box<ListAtom<$name>>),
            /// 配对记号.
            Pair {
                first: Box<$name>,
                second: Box<$name>,
            },
            /// Data 构造.
            Data(Box<DataAtom<$name>>),
            FStr(Box<[$fstr]>),
            $($extra)*
        }
    };
}

pub use crate::define_atom;

//! CPS IR 与线性 IR 共有的原子结构与格式化字符串片段.
//!
//! 两套 IR 的字面量与复合值形态一致, 仅变量引用不同. 共享定义避免新增
//! 字面量形态时两处漂移; 变量相关的语义仍留在各自 IR 中.

/// 格式化字符串片段; 插值值由使用方的 IR 原子类型填充.
#[derive(Debug, Clone)]
pub enum FStrPart<V> {
    Text(String),
    Interp { value: V, spec: Option<String> },
}

/// 定义一套 IR 原子: 共有字面量与复合值, 加上调用方给出的变量变体.
macro_rules! define_atom {
    ($name:ident, $fstr:ty, $($extra:tt)*) => {
        /// IR 原子值; 复合值的子项均已求值为原子.
        #[derive(Debug, Clone)]
        pub enum $name {
            I64(i64),
            U8(u8),
            U64(u64),
            F64(f64),
            Char(char),
            Str(Arc<str>),
            Bool(bool),
            Nil,
            /// 内置函数值; 编号在降级期解析.
            Builtin(BuiltinId),
            /// 列表字面量; `elem` 为元素类型.
            List {
                elem: Type,
                items: Vec<$name>,
            },
            /// 配对记号; `ty` 为完整配对类型.
            Pair {
                ty: Type,
                first: Box<$name>,
                second: Box<$name>,
            },
            Data {
                ty: Symbol,
                variant: Symbol,
                fields: Vec<$name>,
            },
            FStr(Vec<$fstr>),
            $($extra)*
        }
    };
}

pub(crate) use define_atom;

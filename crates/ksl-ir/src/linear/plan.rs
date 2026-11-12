//! 执行视图: 线性 IR 去掉展示信息后的形态.
//!
//! 执行器只依赖 `ksl-plan` 中的类型; 槽位名与类型, 数据声明只存在于 [`Module`].
//! 转换消费模块, 展示数据随之释放.

use std::sync::Arc;

pub use ksl_plan::{Block, Function, Functions, Program};

use super::model::{Block as IrBlock, Function as IrFunction, Module};

impl Module {
    /// 转为执行视图; 消费模块, 槽位名, 类型与数据声明随之释放.
    #[must_use]
    pub fn into_plan(self) -> Program {
        let Module {
            functions, main, ..
        } = self;
        let main_free_vars = main.free_vars.clone().into_boxed_slice();
        let main = Arc::new(convert_function(main));
        let mut table = Functions::new();
        for function in functions {
            let name = function.name.clone();
            table.insert(name, Arc::new(convert_function(function)));
        }
        Program {
            functions: Arc::new(table),
            main,
            main_free_vars,
        }
    }
}

fn convert_function(function: IrFunction) -> Function {
    Function {
        entry: function.entry,
        blocks: function.blocks.into_iter().map(convert_block).collect(),
    }
}

fn convert_block(block: IrBlock) -> Block {
    Block {
        param_count: block.param_count,
        slot_count: block.slots.len(),
        instrs: block.instrs.into_boxed_slice(),
        term: block.term,
    }
}

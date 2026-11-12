; ksl 局部变量查询.
;
; 记录绑定与引用, 使同一名字在各处颜色一致.
; 作用域按词法块划分: 顶层为全局; 绑定, 块与函数类形式开新作用域.
;
; 定义捕获带高亮名后缀, 后缀与 highlights.scm 的捕获名一致.
; Helix 按后缀取样式; 缺少后缀的捕获没有样式, 会把节点已有的颜色抹掉.

; 作用域起点.
(source_file) @local.scope
(let_form) @local.scope
(block_form) @local.scope
(fn_form) @local.scope
(def_form) @local.scope
(handler_form) @local.scope
(main_form) @local.scope
(fix_form) @local.scope

; 定义.
(binding_target name: (identifier) @local.definition.variable)
(param name: (identifier) @local.definition.variable.parameter)
(def_form name: (identifier) @local.definition.function)
(data_form name: (identifier) @local.definition.type)
(data_variant name: (identifier) @local.definition.constructor)
(export_type name: (identifier) @local.definition.type)
(qualified_pattern variant: (identifier) @local.definition.constructor)
(unqualified_pattern variant: (identifier) @local.definition.constructor)
; 模式字段是绑定.
(pattern_field (identifier) @local.definition.variable.parameter)
; 导入的名字在当前模块成为本地名字; 重命名取配对的第二项.
(use_item (identifier) @local.definition.variable)
(use_item (pair (identifier) . (identifier) @local.definition.variable))
; 类型重命名得到的是类型.
(use_item "^" (pair (identifier) . (identifier) @local.definition.type))

; 引用.
(identifier) @local.reference

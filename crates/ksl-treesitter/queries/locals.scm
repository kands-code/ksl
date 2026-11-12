; ksl 局部变量查询.
;
; 记录绑定与引用, 使同一名字在各处颜色一致.
; 作用域按词法块划分: 顶层为全局; 绑定, 块与函数类形式开新作用域.

; 作用域起点.
(source_file) @local.scope
(let_form) @local.scope
(block_form) @local.scope
(fn_form) @local.scope
(def_form) @local.scope
(handler_form) @local.scope
(fix_form) @local.scope

; 定义.
(binding_target name: (identifier) @local.definition)
(param name: (identifier) @local.definition)
(def_form name: (identifier) @local.definition)
(data_form name: (identifier) @local.definition)
(data_variant name: (identifier) @local.definition)
(export_type name: (identifier) @local.definition)
(qualified_pattern variant: (identifier) @local.definition)
(unqualified_pattern variant: (identifier) @local.definition)

; 引用.
(identifier) @local.reference

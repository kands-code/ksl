; 缩进域.
;
; Helix 从光标位置向上找缩进域: 域跨行时, 换行后的缩进比该域起始行多一级.
; ksl 的方括号与花括号是匿名记号, 没有单独包裹内容的节点, 故以整个构造为域.
;
; 同一行开始的多个域只计一次, 见 helix-core 的 `Indentation::add_capture`,
; 故 `@Def[..., @Match[...` 这类同行嵌套不会叠加成两级.

[
  (special_form)
  (call)
  (list_literal)
  (pair)
  (param_list)
  (generic_type)
  (fn_type)
  (data_variant)
  (match_arm)
  (qualified_pattern)
  (unqualified_pattern)
] @indent

; 行首的右括号退回一级, 与开括号所在行对齐.
; 该捕获只对行首记号生效, 故 `}]];` 这类连写只退一级.
[
  "]"
  "}"
  ")"
] @outdent

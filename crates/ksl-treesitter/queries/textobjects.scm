; 文本对象: 供 Helix 的 `mi` / `ma` 选择与 `]f` `]t` 跳转使用.
;
; 捕获名成对出现: `.inside` 是内部, `.around` 连同边界. 客户端取包含光标的
; 最小节点, 故重叠的规则无需额外限定.

; 函数: @Def 是具名定义, @Fn 与 @Handler 是匿名函数与处理器.
[
  (def_form)
  (fn_form)
  (handler_form)
] @function.around

(def_form
  (param_list)
  (_)
  (_) @function.inside)

(fn_form
  (param_list)
  (_)
  (_) @function.inside)

(handler_form
  (param_list)
  (_)
  (_) @function.inside)

; 类型定义: @Data. ksl 没有单独的体节点, 内部选区与整体相同.
(data_form) @class.around
(data_form) @class.inside

; 形参: 内部是单个形参, 整体再带上其后的逗号.
(param_list
  ((param) @parameter.inside . ","? @parameter.around) @parameter.around)

; 注释.
(comment) @comment.inside
(comment)+ @comment.around

; 条目: 变体声明, 匹配分支与列表元素.
(data_variant) @entry.around
(match_arm) @entry.around
(list_literal (_) @entry.around)

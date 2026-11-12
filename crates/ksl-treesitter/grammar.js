/**
 * ksl 语言的 tree-sitter 语法.
 *
 * 语法只描述形态, 语义以 ksl-frontend 的词法与语法分析为准. 两侧的记号流必须一致,
 * 由 `tests/drift.rs` 逐记号比对.
 *
 * - 只有前缀调用, 没有中缀表达式与优先级, 故无需 `prec` 消歧.
 * - 方括号是唯一调用括号; 冒号只出现在类型标注与函数类型位置, 上下文可区分.
 * - 特殊形式名与字面关键字经关键字提取处理, 普通位置的同名标识符不受影响.
 * - 原始字符串与格式化字符串交由外部扫描器识别, 见 `src/scanner.c`.
 */

/// 列表操作族.
const LIST_OPS = [
  'ListGet', 'ListUpdate', 'ListPrepend', 'ListAppend', 'ListConcatenate',
  'ListHead', 'ListTail', 'ListTake', 'ListDrop', 'ListTakeWhile',
  'ListDropWhile', 'ListMap', 'ListMapIndexed', 'ListFilter', 'ListFold',
  'ListPrefixFold', 'ListAny', 'ListAll', 'ListLength', 'IsListEmpty',
  'ListReverse',
  'ListRepeat', 'ListFlatten', 'ListChunk', 'ListRiffle', 'ListWindow',
  'ListZip', 'ListZipWith',
];

/// 配对操作族.
const PAIR_OPS = ['Fst', 'Snd'];

/// Dict 操作族.
const DICT_OPS = [
  'DictLookup', 'DictGet', 'DictPut', 'DictUpdate', 'DictRemove', 'DictHas',
  'DictLength', 'IsDictEmpty', 'DictEntries', 'DictKeys', 'DictValues',
  'DictMerge',
  'DictFilter', 'DictFromEntries',
];

/// Set 操作族.
const SET_OPS = [
  'SetInsert', 'SetRemove', 'SetHas', 'SetLength', 'IsSetEmpty', 'SetItems',
  'SetUnion',
  'SetIntersect', 'SetDifference', 'SetFilter', 'SetFromList',
];

/// Option 与 Result 操作族.
const OPTION_OPS = [
  'IsSome', 'IsNone', 'IsOk', 'IsErr', 'OptionMap', 'ResultMap',
  'OptionAndThen', 'ResultAndThen', 'OptionGet', 'ResultGet',
  'ResultOrElse',
];

/// 命名注册表操作族.
const REGISTRY_OPS = ['Register', 'WhereIs', 'Unregister'];

/// 标识符: 零个或多个下划线, 接一个 XID_Start 字符, 其后为 XID_Continue 字符.
///
/// 纯下划线序列不是标识符, 而是丢弃记号 `_`.
const IDENTIFIER = new RustRegex('_*\\p{XID_Start}\\p{XID_Continue}*');

/// 转义序列: 与词法器的转义集合一致, 依次为 n, t, r, 0, 反斜杠, 双引号,
/// 单引号与 `\u{...}`.
///
/// `\u{...}` 只校验十六进制数字, 标量值范围由词法层判定.
const ESCAPE = '\\\\(?:n|t|r|0|\\\\|"|\'|u\\{[0-9A-Fa-f]+\\})';

/// 逗号分隔的非空序列.
function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}

/// 允许为空的逗号分隔序列.
function commaSep(rule) {
  return optional(commaSep1(rule));
}

module.exports = grammar({
  name: 'ksl',

  // 关键字提取的基础记号; 特殊形式名与字面关键字由此派生.
  word: $ => $.identifier,

  extras: $ => [/\s/, $.comment],

  externals: $ => [$.raw_string, $.fstring],

  rules: {
    // 顶层形式以分号分隔, 最后一条可省略分号.
    source_file: $ => seq(repeat(seq($._expr, ';')), optional($._expr)),

    _expr: $ => choice(
      $.call,
      $.identifier,
      $.number,
      $.string,
      $.char_literal,
      $.fstring,
      $.raw_string,
      $.list_literal,
      $.pair,
      $.special_form,
      $.true_literal,
      $.false_literal,
      $.nil_literal,
    ),

    comment: $ => token(new RustRegex('#.*')),

    identifier: $ => IDENTIFIER,
    // 丢弃记号: 纯下划线序列整体是一个记号, 与词法器一致.
    discard: $ => /_+/,

    number: $ => choice($.integer, $.float, $.u8_integer, $.u64_integer),
    // 一元正负号是字面值前缀, 须紧贴数字. 无符号字面量只允许 `+` 前缀.
    // 十六进制前缀恒为小写, 数字大小写均可.
    integer: $ => token(/[+-]?(0x[0-9a-fA-F]+|0b[01]+|[0-9]+)/),
    float: $ => token(
      /[+-]?([0-9]+\.[0-9]+([eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+)/
    ),
    u8_integer: $ => token(/\+?(0x[0-9a-fA-F]+|0b[01]+|[0-9]+)u/),
    u64_integer: $ => token(/\+?(0x[0-9a-fA-F]+|0b[01]+|[0-9]+)U/),

    string: $ => token(new RustRegex(`"([^"\\\\]|${ESCAPE})*"`)),
    // 字符字面量恰好包含一个字符或一个转义.
    char_literal: $ => token(new RustRegex(`'([^'\\\\]|${ESCAPE})'`)),

    true_literal: $ => 'true',
    false_literal: $ => 'false',
    nil_literal: $ => 'nil',

    // 调用 `f[x, y]`: 被调用者可以是任意表达式, 后缀调用可连写多组.
    // 类型限定名 `Type/Constructor` 只作被调用者, 单独出现不是表达式.
    call: $ => seq(
      field('function', $._callee),
      '[', commaSep($._expr), ']',
    ),
    _callee: $ => choice($.qualified_name, $._expr),
    qualified_name: $ => seq(
      field('qualifier', $.identifier),
      '/',
      field('name', $.identifier),
    ),

    // 列表字面量 `{a, b}` 与配对记号 `(a, b)`.
    list_literal: $ => seq('{', commaSep($._expr), '}'),
    pair: $ => seq('(', $._expr, ',', $._expr, ')'),

    special_form: $ => choice(
      $.let_form, $.block_form, $.if_form, $.which_form, $.fn_form, $.def_form, $.exit_form,
      $.handler_form, $.main_form, $.continue_form, $.spawn_form, $.send_form, $.receive_form,
      $.join_form, $.kill_form, $.monitor_form, $.unmonitor_form, $.pid_serial_form,
      $.set_timer_form, $.cancel_timer_form, $.halt_form, $.sleep_form, $.fix_form,
      $.data_form, $.export_form, $.use_form, $.link_form, $.match_form,
      $.list_op_form, $.pair_op_form, $.dict_form, $.set_form, $.dict_op_form, $.set_op_form,
      $.registry_form, $.option_op_form,
      $.assert_form, $.panic_form, $.todo_form, $.unreachable_form,
    ),

    let_form: $ => seq('@', 'Let', '[', $.binding_target, ',', $._expr, ']'),
    // 绑定位为名字或 `_`, 可带类型标注.
    binding_target: $ => choice(
      seq(field('name', $.identifier), ':', field('type', $._type)),
      $.identifier,
      $.discard,
    ),

    block_form: $ => seq('@', 'Block', '[', commaSep($._expr), ']'),
    if_form: $ => seq('@', 'If', '[', $._expr, ',', $._expr, ',', $._expr, ']'),
    which_form: $ => seq(
      '@', 'Which', '[', '{', commaSep($.which_arm), '}', ',', $._expr, ']',
    ),
    which_arm: $ => seq('(', $._expr, ',', $._expr, ')'),
    fn_form: $ => seq('@', 'Fn', '[', $.param_list, ':', $._type, ',', $._expr, ']'),
    def_form: $ => seq(
      '@', 'Def', '[', field('name', $.identifier), ',',
      $.param_list, ':', $._type, ',', $._expr, ']',
    ),
    exit_form: $ => seq('@', 'Exit', '[', $._expr, ']'),
    handler_form: $ => seq(
      '@', 'Handler', '[', $.param_list, ':', $._type, ',', $._expr, ']',
    ),
    main_form: $ => seq(
      '@', 'Main', '[', $.param_list, ':', $._type, ',', $._expr, ']',
    ),
    continue_form: $ => seq('@', 'Continue', '[', commaSep($._expr), ']'),
    spawn_form: $ => seq(
      '@', choice('Spawn', 'SpawnDetached'),
      '[', $._expr, repeat(seq(',', $._expr)), ']',
    ),
    send_form: $ => seq('@', 'Send', '[', $._expr, ',', $._expr, ',', $._expr, ']'),
    receive_form: $ => seq('@', 'Receive', '[', $._expr, ',', $._expr, ']'),
    join_form: $ => seq('@', 'Join', '[', $._expr, ']'),
    kill_form: $ => seq('@', 'Kill', '[', $._expr, ']'),
    monitor_form: $ => seq('@', 'Monitor', '[', $._expr, ']'),
    unmonitor_form: $ => seq('@', 'Unmonitor', '[', $._expr, ']'),
    pid_serial_form: $ => seq('@', 'PidSerial', '[', $._expr, ']'),
    set_timer_form: $ => seq(
      '@', 'SetTimer', '[', $._expr, ',', $._expr, ',', $._expr, ',', $._expr, ']',
    ),
    cancel_timer_form: $ => seq('@', 'CancelTimer', '[', $._expr, ']'),
    halt_form: $ => seq('@', 'Halt', '[' , ']'),
    sleep_form: $ => seq('@', 'Sleep', '[', $._expr, ']'),
    fix_form: $ => seq('@', 'Fix', '[', commaSep($.def_form), ']'),

    // 断言与主动中止形式.
    assert_form: $ => seq('@', 'Assert', '[', $._expr, ',', $._expr, ']'),
    panic_form: $ => seq('@', 'Panic', '[', $._expr, ']'),
    todo_form: $ => seq('@', 'Todo', '[', $._expr, ']'),
    unreachable_form: $ => seq('@', 'Unreachable', '[', $._expr, ']'),

    data_form: $ => seq(
      '@', 'Data', '[', field('name', $.identifier), ',',
      '{', commaSep($.data_variant), '}', ']',
    ),
    data_variant: $ => seq(field('name', $.identifier), '[', commaSep($._type), ']'),

    export_form: $ => seq(
      '@', 'Export', '[', commaSep(choice($.identifier, $.export_type)), ']',
    ),
    export_type: $ => seq('^', field('name', $.identifier)),

    use_form: $ => seq(
      '@', 'Use', '[', '{', commaSep($.use_item), '}', ',', $._path, ']',
    ),
    // 模块路径: 普通字符串或原始字符串.
    _path: $ => choice($.string, $.raw_string),
    use_item: $ => choice(
      $.identifier,
      $.export_type,
      $.pair,
      seq('^', $.pair),
    ),

    link_form: $ => seq(
      '@', 'Link', '[', $._path, ',',
      '{', commaSep($.export_type), '}', ',',
      '{', commaSep($.link_value), '}', ']',
    ),
    link_value: $ => seq(field('name', $.identifier), ':', $._type),

    match_form: $ => seq(
      '@', 'Match', '[', $._expr, ',', '{', commaSep($.match_arm), '}', ']',
    ),
    match_arm: $ => seq('(', $._pattern, ',', $._expr, ')'),
    // 模式分带类型限定与省略类型两种形态. 省略形态以 `/` 开头, 故各自成一条规则.
    _pattern: $ => choice($.discard, $.qualified_pattern, $.unqualified_pattern),
    qualified_pattern: $ => seq(
      field('qualifier', $.identifier),
      '/',
      field('variant', $.identifier),
      '[', commaSep($.pattern_field), ']',
    ),
    unqualified_pattern: $ => seq(
      '/',
      field('variant', $.identifier),
      '[', commaSep($.pattern_field), ']',
    ),
    pattern_field: $ => choice($.identifier, $.discard),

    list_op_form: $ => seq('@', choice(...LIST_OPS), '[', commaSep($._expr), ']'),
    pair_op_form: $ => seq('@', choice(...PAIR_OPS), '[', commaSep($._expr), ']'),
    dict_form: $ => seq('@', 'Dict', '[', commaSep($._expr), ']'),
    set_form: $ => seq('@', 'Set', '[', commaSep($._expr), ']'),
    dict_op_form: $ => seq('@', choice(...DICT_OPS), '[', commaSep($._expr), ']'),
    set_op_form: $ => seq('@', choice(...SET_OPS), '[', commaSep($._expr), ']'),
    registry_form: $ => seq(
      '@', choice(...REGISTRY_OPS), '[', commaSep($._expr), ']',
    ),
    option_op_form: $ => seq('@', choice(...OPTION_OPS), '[', commaSep($._expr), ']'),

    // 形参列表 `[x : i64, ...]`.
    param_list: $ => seq('[', commaSep($.param), ']'),
    param: $ => seq(field('name', $.identifier), ':', field('type', $._type)),

    // 类型表达式: 具名类型, 参数化容器或函数类型.
    // `Fn` 是关键字, 类型位置须显式接纳, 否则裸 `Fn` 无法作为具名类型解析.
    _type: $ => choice(
      alias($.identifier, $.type_name),
      alias('Fn', $.type_name),
      $.generic_type,
      $.fn_type,
    ),
    generic_type: $ => seq(field('name', $.identifier), '[', commaSep($._type), ']'),
    fn_type: $ => seq('Fn', '[', '[', commaSep($._type), ']', ':', $._type, ']'),
  },
});

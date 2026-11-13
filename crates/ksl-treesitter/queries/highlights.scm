; ksl 语法高亮查询.
;
; 约定: 兜底规则在前, 具体规则在后; 同一节点被多条规则捕获时, 后者生效.

; 变量: 兜底规则, 其余标识符规则在其后覆盖.
(identifier) @variable

; 特殊形式名.
[
  "Let"
  "Block"
  "If"
  "Fn"
  "Def"
  "Exit"
  "Handler"
  "Main"
  "Continue"
  "Spawn"
  "SpawnDetached"
  "Send"
  "Receive"
  "Join"
  "Kill"
  "Monitor"
  "Unmonitor"
  "PidSerial"
  "SetTimer"
  "CancelTimer"
  "Halt"
  "Sleep"
  "Fix"
  "Data"
  "Export"
  "Use"
  "Match"
  "Dict"
  "Set"
  "Register"
  "WhereIs"
  "Unregister"
  "Assert"
  "Panic"
  "Todo"
  "Unreachable"
] @keyword

; 列表操作族.
[
  "ListGet"
  "ListUpdate"
  "ListPrepend"
  "ListAppend"
  "ListConcatenate"
  "ListHead"
  "ListTail"
  "ListTake"
  "ListDrop"
  "ListReverse"
  "ListRepeat"
  "ListFlatten"
  "ListChunk"
  "ListTakeWhile"
  "ListDropWhile"
  "ListMap"
  "ListMapIndexed"
  "ListFilter"
  "ListFold"
  "ListPrefixFold"
  "ListAny"
  "ListAll"
  "ListLength"
  "ListRiffle"
  "ListWindow"
  "ListZip"
  "ListZipWith"
  "Fst"
  "Snd"
  "DictLookup"
  "DictGet"
  "DictPut"
  "DictUpdate"
  "DictRemove"
  "DictHas"
  "DictLength"
  "DictEntries"
  "DictKeys"
  "DictValues"
  "DictMerge"
  "DictFilter"
  "DictFromEntries"
  "SetInsert"
  "SetRemove"
  "SetHas"
  "SetLength"
  "SetItems"
  "SetUnion"
  "SetIntersect"
  "SetDifference"
  "SetFilter"
  "SetFromList"
] @keyword.operator

; Option 与 Result 操作族.
[
  "IsSome"
  "IsNone"
  "IsOk"
  "IsErr"
  "OptionMap"
  "ResultMap"
  "OptionAndThen"
  "ResultAndThen"
  "OptionGet"
  "ResultGet"
] @keyword.operator

; 字面量关键字.
[
  (true_literal)
  (false_literal)
  (nil_literal)
] @constant.builtin

; 类型.
(type_name) @type
(generic_type name: (identifier) @type)
(fn_type) @type
(export_type name: (identifier) @type)
(use_item "^" (pair (identifier) @type (identifier) @type))

; 函数与构造器.
(call function: (identifier) @function)
(qualified_name qualifier: (identifier) @type)
(qualified_name name: (identifier) @constructor)
(qualified_pattern variant: (identifier) @constructor)
(unqualified_pattern variant: (identifier) @constructor)
(data_variant name: (identifier) @constructor)

; 内置函数.
;
; 名单由 `scripts/gen-builtins.sh` 生成, 一致性由 `tests/names.rs` 校验.
; 内置名是保留名, 不能用作绑定, 故按名字匹配不会误伤同名局部变量.
;
; BEGIN builtins
((identifier) @function.builtin
 (#any-of? @function.builtin
  "AbsF64"
  "AbsI64"
  "AddDay"
  "AddF64"
  "AddHour"
  "AddI64"
  "AddMillisecond"
  "AddMinute"
  "AddMonth"
  "AddSecond"
  "AddU64"
  "AddU8"
  "AddWeek"
  "AddYear"
  "And"
  "AppendFile"
  "AppendFileBytes"
  "ArcCos"
  "ArcCosh"
  "ArcSin"
  "ArcSinh"
  "ArcTan"
  "ArcTan2"
  "ArcTanh"
  "Args"
  "BitAndI64"
  "BitAndU64"
  "BitAndU8"
  "BitOrI64"
  "BitOrU64"
  "BitOrU8"
  "BitReverseI64"
  "BitReverseU64"
  "BitReverseU8"
  "BitXorI64"
  "BitXorU64"
  "BitXorU8"
  "Ceiling"
  "CharToU64"
  "CharsToString"
  "Close"
  "ClosedFile"
  "CopyFile"
  "Cos"
  "Cosh"
  "CreateDirectory"
  "CreateDirectoryAll"
  "DateDay"
  "DateMonth"
  "DateTimeFromParts"
  "DateTimeInZone"
  "DateTimeToEpochMillis"
  "DateTimeToString"
  "DateWeekday"
  "DateYear"
  "DecodeUtf8"
  "DivideF64"
  "DivideI64"
  "DivideU64"
  "DivideU8"
  "EncodeUtf8"
  "EpochMillisToDateTime"
  "EqualBool"
  "EqualChar"
  "EqualDateTime"
  "EqualF64"
  "EqualI64"
  "EqualString"
  "EqualU64"
  "EqualU8"
  "Floor"
  "Flush"
  "GetCurrentDirectory"
  "GetEnv"
  "GreaterDateTime"
  "GreaterF64"
  "GreaterI64"
  "GreaterOrEqualDateTime"
  "GreaterOrEqualF64"
  "GreaterOrEqualI64"
  "GreaterOrEqualU64"
  "GreaterOrEqualU8"
  "GreaterU64"
  "GreaterU8"
  "I64ToF64"
  "I64ToU64"
  "I64ToU8"
  "IsAlphanumeric"
  "IsAsciiDigit"
  "IsDirectory"
  "IsFile"
  "IsLetter"
  "IsLowercase"
  "IsPathExists"
  "IsUppercase"
  "IsWhitespace"
  "JoinPath"
  "LessDateTime"
  "LessF64"
  "LessI64"
  "LessOrEqualDateTime"
  "LessOrEqualF64"
  "LessOrEqualI64"
  "LessOrEqualU64"
  "LessOrEqualU8"
  "LessU64"
  "LessU8"
  "ListDirectory"
  "ModuloI64"
  "ModuloU64"
  "ModuloU8"
  "MulF64"
  "MulI64"
  "MulU64"
  "MulU8"
  "Not"
  "Now"
  "NowUtc"
  "OpenAppend"
  "OpenRead"
  "OpenWrite"
  "Or"
  "ParseDateTime"
  "ParseF64"
  "ParseI64"
  "ParseU64"
  "Power"
  "Print"
  "PrintError"
  "PrintRaw"
  "QuotientI64"
  "ReadChunk"
  "ReadFile"
  "ReadFileBytes"
  "ReadKey"
  "ReadLine"
  "ReadLineFrom"
  "ReadLinePlain"
  "RemainderI64"
  "RemoveDirectory"
  "RemoveFile"
  "RenamePath"
  "Round"
  "ShiftLeftI64"
  "ShiftLeftU64"
  "ShiftLeftU8"
  "ShiftRightI64"
  "ShiftRightU64"
  "ShiftRightU8"
  "Sin"
  "Sinh"
  "SocketClose"
  "SocketPeer"
  "SocketRead"
  "SocketReadAll"
  "SocketReadExact"
  "SocketReadLine"
  "SocketWrite"
  "SocketWriteAll"
  "SocketWriteLine"
  "Sqrt"
  "StringContains"
  "StringEndsWith"
  "StringJoin"
  "StringLength"
  "StringLines"
  "StringLowercase"
  "StringSplit"
  "StringStartsWith"
  "StringToChars"
  "StringTrim"
  "StringTrimEnd"
  "StringTrimStart"
  "StringUppercase"
  "StringWords"
  "SubDay"
  "SubF64"
  "SubHour"
  "SubI64"
  "SubMillisecond"
  "SubMinute"
  "SubMonth"
  "SubSecond"
  "SubU64"
  "SubU8"
  "SubWeek"
  "SubYear"
  "Tan"
  "Tanh"
  "TcpAccept"
  "TcpCloseListener"
  "TcpConnect"
  "TcpListen"
  "TcpLocalPort"
  "TimeHour"
  "TimeMillisecond"
  "TimeMinute"
  "TimeSecond"
  "TimeZoneName"
  "TimeZoneOffsetMinutes"
  "Truncate"
  "U64ToChar"
  "U64ToF64"
  "U64ToI64"
  "U64ToU8"
  "U8ToI64"
  "U8ToU64"
  "WriteBytes"
  "WriteFile"
  "WriteFileBytes"
  "WriteString"
))
; END builtins

; 形参与模式字段.
(param name: (identifier) @variable.parameter)
(pattern_field (identifier) @variable.parameter)

; 字面量.
;
; 数字与字符用具名捕获. Helix, nvim 与 Zed 按点分前缀逐级回退, 故
; `constant.numeric.integer` 缺失时会落到上层样式; 单段名字无处可退.
(integer) @constant.numeric.integer
(u8_integer) @constant.numeric.integer
(u64_integer) @constant.numeric.integer
(float) @constant.numeric.float
(string) @string
(char_literal) @constant.character
(fstring) @string
(raw_string) @string

; 注释.
(comment) @comment

; 丢弃记号 `_`, 语义是"该值不使用". 沿用 Helix 对 Rust 中 `_` 的标法.
(discard) @comment.unused

; 标记.
;
; `@` 是特殊形式前缀, `^` 是类型提升前缀, `/` 是受限名字的分隔符.
; 三者都不是运算, 按标点处理.
[
  "@"
  "^"
  "/"
] @punctuation.special

[
  ","
  ";"
  ":"
] @punctuation.delimiter

[
  "["
  "]"
  "{"
  "}"
  "("
  ")"
] @punctuation.bracket

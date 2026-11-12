// ksl 的 tree-sitter 外部扫描器.
//
// 只处理无法用正则描述的两类记号: 原始字符串与格式化字符串.
//
// 外部扫描器在跳过空白之前被调用. 扫描失败时返回 false,
// 由 tree-sitter 回退到扫描起点后交给内部词法规则.

#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <stdint.h>

// 外部记号顺序须与 grammar.js 的 externals 数组一致.
enum TokenType {
  RAW_STRING,
  FSTRING,
};

void *tree_sitter_ksl_external_scanner_create(void) { return NULL; }

void tree_sitter_ksl_external_scanner_destroy(void *payload) { (void)payload; }

unsigned tree_sitter_ksl_external_scanner_serialize(void *payload,
                                                    char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_ksl_external_scanner_deserialize(void *payload,
                                                  const char *buffer,
                                                  unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

static inline bool at_eof(TSLexer *lexer) { return lexer->eof(lexer); }

static inline bool is_hex_digit(int32_t c) {
  return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') ||
         (c >= 'A' && c <= 'F');
}

// 转义序列: 与词法器的转义集合一致, 依次为 n, t, r, 0, 反斜杠, 双引号,
// 单引号与 `\u{...}`.
//
// 标量值范围不在此处判定, 由词法层负责.
static bool scan_escape(TSLexer *lexer) {
  advance(lexer);
  int32_t c = lexer->lookahead;
  if (c == 'n' || c == 't' || c == 'r' || c == '0' || c == '\\' || c == '"' ||
      c == '\'') {
    advance(lexer);
    return true;
  }
  if (c == 'u') {
    advance(lexer);
    if (lexer->lookahead != '{') {
      return false;
    }
    advance(lexer);
    uint32_t digits = 0;
    while (is_hex_digit(lexer->lookahead)) {
      advance(lexer);
      digits++;
    }
    if (digits == 0 || lexer->lookahead != '}') {
      return false;
    }
    advance(lexer);
    return true;
  }
  return false;
}

// 原始字符串正文: 已越过开引号, 收尾须为引号加同样数量的 '#'.
static bool scan_raw_body(TSLexer *lexer, uint32_t hashes) {
  for (;;) {
    if (at_eof(lexer)) {
      return false;
    }
    if (lexer->lookahead == '"') {
      advance(lexer);
      uint32_t count = 0;
      // 收尾井号数量须与开引号前的一致; 不一致时已消费的字符都计入正文.
      while (lexer->lookahead == '#') {
        advance(lexer);
        count++;
      }
      if (count == hashes) {
        return true;
      }
      continue;
    }
    advance(lexer);
  }
}

// 插值正文: 扫描到未嵌套的 '}' 为止; 内部允许字符串与原始字符串.
static bool scan_interpolation(TSLexer *lexer) {
  for (;;) {
    if (at_eof(lexer)) {
      return false;
    }
    int32_t c = lexer->lookahead;
    if (c == '}') {
      advance(lexer);
      return true;
    }
    if (c == '{') {
      // 插值内不允许嵌套花括号.
      return false;
    }
    if (c == '\'' || c == '"') {
      int32_t quote = c;
      advance(lexer);
      for (;;) {
        if (at_eof(lexer)) {
          return false;
        }
        int32_t d = lexer->lookahead;
        if (d == '\\') {
          advance(lexer);
          if (!at_eof(lexer)) {
            advance(lexer);
          }
          continue;
        }
        advance(lexer);
        if (d == quote) {
          break;
        }
      }
      continue;
    }
    if (c == 'r') {
      // 'r' 可能开启原始字符串. 无论是否匹配, 已消费的字符都计入正文.
      advance(lexer);
      uint32_t hashes = 0;
      while (lexer->lookahead == '#') {
        advance(lexer);
        hashes++;
      }
      if (lexer->lookahead == '"') {
        advance(lexer);
        if (!scan_raw_body(lexer, hashes)) {
          return false;
        }
      }
      continue;
    }
    advance(lexer);
  }
}

// 原始字符串: `r"..."`, `r#"..."#`, 井号数量任意.
static bool scan_raw_string(TSLexer *lexer) {
  advance(lexer);
  uint32_t hashes = 0;
  while (lexer->lookahead == '#') {
    advance(lexer);
    hashes++;
  }
  if (lexer->lookahead != '"') {
    return false;
  }
  advance(lexer);
  return scan_raw_body(lexer, hashes);
}

// 格式化字符串: `f"..."`, 插值以花括号界定, 双花括号为转义.
static bool scan_fstring(TSLexer *lexer) {
  advance(lexer);
  if (lexer->lookahead != '"') {
    return false;
  }
  advance(lexer);
  for (;;) {
    if (at_eof(lexer)) {
      return false;
    }
    int32_t c = lexer->lookahead;
    if (c == '"') {
      advance(lexer);
      return true;
    }
    if (c == '\\') {
      if (!scan_escape(lexer)) {
        return false;
      }
      continue;
    }
    if (c == '{') {
      advance(lexer);
      if (lexer->lookahead == '{') {
        advance(lexer);
        continue;
      }
      if (!scan_interpolation(lexer)) {
        return false;
      }
      continue;
    }
    if (c == '}') {
      // 落单的 '}' 必须写作 '}}'.
      advance(lexer);
      if (lexer->lookahead == '}') {
        advance(lexer);
        continue;
      }
      return false;
    }
    advance(lexer);
  }
}

bool tree_sitter_ksl_external_scanner_scan(void *payload, TSLexer *lexer,
                                           const bool *valid_symbols) {
  (void)payload;

  // 外部扫描器在跳过空白之前被调用, 故须自行跳过前导空白.
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
         lexer->lookahead == '\r' || lexer->lookahead == '\n') {
    lexer->advance(lexer, true);
  }

  if (valid_symbols[RAW_STRING] && lexer->lookahead == 'r') {
    if (scan_raw_string(lexer)) {
      lexer->result_symbol = RAW_STRING;
      return true;
    }
    return false;
  }

  if (valid_symbols[FSTRING] && lexer->lookahead == 'f') {
    if (scan_fstring(lexer)) {
      lexer->result_symbol = FSTRING;
      return true;
    }
    return false;
  }

  return false;
}

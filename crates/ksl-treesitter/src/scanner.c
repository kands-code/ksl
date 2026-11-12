// ksl 的 tree-sitter 外部扫描器.
//
// 只处理无法用正则描述的两类记号: 原始字符串与格式化字符串.
//
// 外部扫描器在跳过空白之前被调用, 故须自行跳过前导空白; 扫描失败时返回 false,
// 由内部词法器重置位置后重新接管.

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

unsigned tree_sitter_ksl_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_ksl_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

static inline bool at_eof(TSLexer *lexer) { return lexer->eof(lexer); }

// 原始字符串正文: 已越过开引号, 收尾须为引号加同样数量的 '#'.
static bool scan_raw_body(TSLexer *lexer, uint32_t hashes) {
  for (;;) {
    if (at_eof(lexer)) {
      return false;
    }
    if (lexer->lookahead == '"') {
      advance(lexer);
      uint32_t count = 0;
      while (lexer->lookahead == '#' && count < hashes) {
        advance(lexer);
        count++;
      }
      if (count == hashes) {
        return true;
      }
      // 数量不合, 继续按正文处理.
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
      // 可能是原始字符串; 已消费的字符无论如何都是正文.
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
  advance(lexer); // 'r'
  uint32_t hashes = 0;
  while (lexer->lookahead == '#') {
    advance(lexer);
    hashes++;
  }
  if (lexer->lookahead != '"') {
    return false;
  }
  advance(lexer); // 开引号
  return scan_raw_body(lexer, hashes);
}

// 格式化字符串: `f"..."`, 插值以花括号界定, 双花括号为转义.
static bool scan_fstring(TSLexer *lexer) {
  advance(lexer); // 'f'
  if (lexer->lookahead != '"') {
    return false;
  }
  advance(lexer); // 开引号
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
      advance(lexer);
      if (!at_eof(lexer)) {
        advance(lexer);
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
      advance(lexer);
      if (lexer->lookahead == '}') {
        advance(lexer);
      }
      continue;
    }
    advance(lexer);
  }
}

bool tree_sitter_ksl_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols) {
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

#ifndef KSL_PLUGIN_H
#define KSL_PLUGIN_H

#include <stddef.h>
#include <stdint.h>

#define KSL_PLUGIN_ABI_VERSION 1

#define KSL_STATUS_OK 0
#define KSL_STATUS_ERROR 1
#define KSL_STATUS_INTERNAL 2

#define KSL_FUNCTION_FALLIBLE 1

#define KSL_EXECUTION_CURRENT 0
#define KSL_EXECUTION_IO 1
#define KSL_EXECUTION_CPU 2

#define KSL_THREAD_ANY 0
#define KSL_THREAD_RUNTIME 1

/* UTF-8 字节切片, 不要求 NUL 结尾. */
typedef struct {
  const uint8_t *ptr;
  size_t len;
} KslStr;

/* KslType.tag 的取值. */
enum {
  KSL_TYPE_I64 = 1,
  KSL_TYPE_U64 = 2,
  KSL_TYPE_U8 = 3,
  KSL_TYPE_F64 = 4,
  KSL_TYPE_BOOL = 5,
  KSL_TYPE_STRING = 6,
  KSL_TYPE_NIL = 7,
  KSL_TYPE_OPAQUE = 8,
  KSL_TYPE_CHAR = 9,
};

/* 一个参数或返回值的类型. */
typedef struct {
  uint32_t tag;
  uint32_t opaque;
} KslType;

/* KslValue.tag 的取值. */
enum {
  KSL_VALUE_NIL = 0,
  KSL_VALUE_I64 = 1,
  KSL_VALUE_U64 = 2,
  KSL_VALUE_U8 = 3,
  KSL_VALUE_F64 = 4,
  KSL_VALUE_BOOL = 5,
  KSL_VALUE_STRING = 6,
  KSL_VALUE_OPAQUE = 7,
  KSL_VALUE_CHAR = 8,
};

typedef struct {
  const uint8_t *ptr;
  size_t len;
} KslString;

typedef struct {
  uint32_t type_index;
  uint32_t owned;
  uint64_t handle;
} KslOpaqueValue;

typedef union {
  int64_t as_i64;
  uint64_t as_u64;
  uint8_t as_u8;
  double as_f64;
  uint8_t as_bool;
  uint32_t as_char;
  KslString as_string;
  KslOpaqueValue as_opaque;
} KslValueData;

typedef struct {
  uint32_t tag;
  KslValueData data;
} KslValue;

typedef struct {
  KslStr name;
  void (*release)(uint64_t handle);
  uint32_t release_thread;
} KslOpaqueType;

/* 返回 KSL_STATUS_OK 表示成功; 业务失败返回 KSL_STATUS_ERROR,
   内部错误或 panic 返回 KSL_STATUS_INTERNAL; 失败时 out 中放 UTF-8 错误信息. */
typedef uint32_t (*KslFunctionEntry)(const KslValue *args, uint32_t argc,
                                     KslValue *out);
typedef uint32_t (*KslGlobalEntry)(KslValue *out);

/* 初始化入口; 失败时把错误信息写入 out. */
typedef uint32_t (*KslInitEntry)(KslValue *out);

/* 卸载入口. */
typedef void (*KslUnloadEntry)(void);

/* 事件泵; 不得阻塞等待下一帧或下一事件. */
typedef void (*KslEventPumpEntry)(void);

typedef struct {
  /* ksl 链接名; 对应 @Link 中的名字. */
  KslStr name;
  /* 插件源名; 对应 Rust 函数名. */
  KslStr source_name;
  const KslType *params;
  uint32_t param_count;
  uint32_t flags;
  KslType ret;
  KslFunctionEntry entry;
  uint32_t execution;
} KslFunctionDef;

typedef struct {
  /* ksl 链接名; 对应 @Link 中的名字. */
  KslStr name;
  /* 插件源名; 对应 Rust 常量或静态项名. */
  KslStr source_name;
  KslType ty;
  KslGlobalEntry get;
  uint32_t execution;
} KslGlobalDef;

typedef struct {
  uint32_t abi_version;
  KslStr name;
  KslStr version;
  const KslOpaqueType *types;
  uint32_t type_count;
  const KslFunctionDef *functions;
  uint32_t function_count;
  const KslGlobalDef *globals;
  uint32_t global_count;
  /* 初始化入口; 描述符校验通过后调用一次. */
  KslInitEntry init;
  /* 卸载入口; 动态库卸载前调用一次. */
  KslUnloadEntry unload;
  uint32_t unload_thread;
  /* 事件泵; 由调度循环在片段之间调用. */
  KslEventPumpEntry pump;
} KslPlugin;

/* 插件导出的唯一入口. 不接受 host_abi_version 时返回空指针.
   返回的描述符须在动态库卸载前保持有效. */
const KslPlugin *ksl_plugin_init(uint32_t host_abi_version);

#endif

#ifndef SX_H
#define SX_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct SxMessage SxMessage;

typedef struct SxErrorInfo {
  int code;
  char *message;
} SxErrorInfo;

typedef struct SxByteBuffer {
  uint8_t *data;
  size_t len;
} SxByteBuffer;

int sx_message_parse_text(const char *input, SxMessage **out_msg, SxErrorInfo **out_err);
int sx_message_to_text(const SxMessage *msg, char **out_text, SxErrorInfo **out_err);
int sx_message_encode_binary(const SxMessage *msg, SxByteBuffer *out_buf, SxErrorInfo **out_err);
int sx_message_decode_binary(const uint8_t *data, size_t len, SxMessage **out_msg, SxErrorInfo **out_err);

void sx_message_free(SxMessage *msg);
void sx_error_free(SxErrorInfo *err);
void sx_string_free(char *text);
void sx_bytes_free(SxByteBuffer buf);

int sx_value_get_type(const SxMessage *msg);
int sx_value_get_field(const SxMessage *msg, const char *field, SxMessage **out_msg, SxErrorInfo **out_err);
int sx_value_get_string(const SxMessage *msg, char **out_text, SxErrorInfo **out_err);
int sx_value_get_i64(const SxMessage *msg, int64_t *out_value);
int sx_value_get_u64(const SxMessage *msg, uint64_t *out_value);
int sx_value_get_bool(const SxMessage *msg, bool *out_value);
int sx_value_get_bytes(const SxMessage *msg, SxByteBuffer *out_buf, SxErrorInfo **out_err);

int sx_hash_logical(const SxMessage *msg, SxByteBuffer *out_buf, SxErrorInfo **out_err);
int sx_apply_delta(const SxMessage *base_msg, const SxMessage *delta_msg, SxMessage **out_msg, SxErrorInfo **out_err);

const char *sx_version(void);

#ifdef __cplusplus
}
#endif

#endif

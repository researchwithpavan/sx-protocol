#include "../sx.h"
#include <stdio.h>

int main(void) {
  SxMessage *msg = NULL;
  SxErrorInfo *err = NULL;
  if (sx_message_parse_text("{name:\"Asha\",active:true}", &msg, &err) != 0) {
    if (err) {
      fprintf(stderr, "error[%d]: %s\n", err->code, err->message);
      sx_error_free(err);
    }
    return 1;
  }

  SxByteBuffer hash = {0};
  if (sx_hash_logical(msg, &hash, &err) == 0) {
    printf("hash bytes: %zu\n", hash.len);
    sx_bytes_free(hash);
  }

  sx_message_free(msg);
  return 0;
}

#include "../../bindings/c/sx.h"
#include <stdio.h>

int main(void) {
  SxMessage *msg = NULL;
  SxErrorInfo *err = NULL;
  if (sx_message_parse_text("{name:\"Asha\"}", &msg, &err) != 0) {
    return 1;
  }
  sx_message_free(msg);
  return 0;
}

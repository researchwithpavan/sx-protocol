#pragma once

#include "../c/sx.h"
#include <stdexcept>
#include <string>
#include <vector>

namespace sx {

class Error : public std::runtime_error {
public:
  explicit Error(const std::string &msg) : std::runtime_error(msg) {}
};

inline void throw_if_error(int status, SxErrorInfo *err) {
  if (status == 0)
    return;
  std::string message = "SX error";
  if (err) {
    message = std::string("SX[") + std::to_string(err->code) + "]: " + (err->message ? err->message : "");
    sx_error_free(err);
  }
  throw Error(message);
}

class Message {
public:
  explicit Message(SxMessage *raw = nullptr) : raw_(raw) {}
  ~Message() {
    if (raw_)
      sx_message_free(raw_);
  }

  Message(const Message &) = delete;
  Message &operator=(const Message &) = delete;

  Message(Message &&other) noexcept : raw_(other.raw_) { other.raw_ = nullptr; }
  Message &operator=(Message &&other) noexcept {
    if (this != &other) {
      if (raw_)
        sx_message_free(raw_);
      raw_ = other.raw_;
      other.raw_ = nullptr;
    }
    return *this;
  }

  static Message parse_text(const std::string &text) {
    SxMessage *msg = nullptr;
    SxErrorInfo *err = nullptr;
    throw_if_error(sx_message_parse_text(text.c_str(), &msg, &err), err);
    return Message(msg);
  }

  std::string to_text() const {
    char *out = nullptr;
    SxErrorInfo *err = nullptr;
    throw_if_error(sx_message_to_text(raw_, &out, &err), err);
    std::string text = out ? out : "";
    sx_string_free(out);
    return text;
  }

  std::vector<unsigned char> to_binary() const {
    SxByteBuffer buf{0};
    SxErrorInfo *err = nullptr;
    throw_if_error(sx_message_encode_binary(raw_, &buf, &err), err);
    std::vector<unsigned char> out(buf.data, buf.data + buf.len);
    sx_bytes_free(buf);
    return out;
  }

private:
  SxMessage *raw_;
};

} // namespace sx

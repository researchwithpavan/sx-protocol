#include "../../bindings/cpp/sx.hpp"

int main() {
  auto msg = sx::Message::parse_text("{name:\"Asha\"}");
  (void)msg.to_text();
  return 0;
}

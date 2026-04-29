#include "../sx.hpp"
#include <iostream>

int main() {
  auto msg = sx::Message::parse_text("{name:\"Asha\",active:true}");
  std::cout << msg.to_text() << std::endl;
  return 0;
}

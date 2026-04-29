# C++ Binding

Header-only RAII wrapper over `bindings/c/sx.h`.

## Example

```cpp
#include "sx.hpp"
#include <iostream>

int main() {
  auto msg = sx::Message::parse_text("{name:\"Asha\"}");
  std::cout << msg.to_text() << "\n";
}
```

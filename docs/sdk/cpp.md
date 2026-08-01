# C++ SDK

## Build and install

The CMake installer builds the Rust FFI library and installs a relocatable SDK
package on Linux or macOS:

```bash
cmake -S . -B build -DCMAKE_INSTALL_PREFIX="$PWD/voxa-sdk"
cmake --build build --parallel
cmake --install build
```

Consume it from an independent CMake project:

```cmake
find_package(Voxa CONFIG REQUIRED)
add_executable(agent main.cpp)
target_link_libraries(agent PRIVATE Voxa::cpp)
```

Configure that project with `-DCMAKE_PREFIX_PATH=/path/to/voxa-sdk`.

## Develop a Node

Derive from `voxa::TransformNode`, implement `on_process`, then use
`voxa::Node::make<T>`. `voxa::TextFrame` owns its input strings so its C ABI
view cannot accidentally outlive temporary data. `voxa::Runtime::run_text`
executes the focused single-node text harness and copies the result into an
owned `std::string`.

See `examples/cpp/uppercase-node`. C++17 is required. The CMake installer is
currently supported on Linux and macOS; Windows import-library packaging is a
documented follow-up.

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

## Register a Graph v1 Factory

```cpp
std::vector<voxa::GraphNodeFactory> factories{
    voxa::GraphNodeFactory::make<Uppercase>("example.cpp.uppercase")};
uint32_t workers = 0;
runtime.run_graph(graph_json, factories, workers, error);
```

The versioned C ABI copies registration strings, asks the trusted C++ factory
for a fresh Node vtable during materialization, and transfers lifecycle
ownership to the Rust concurrent Runtime. C++ exceptions remain contained by
the existing `noexcept` trampolines.

For multimodal nodes, derive from `voxa::MultimodalGraphNode`, return
`std::vector<voxa::GraphEmission>`, and register a
`voxa::MultimodalGraphNodeFactory` with a kind, ports JSON, and config schema.
`Runtime::run_multimodal_graph` uses the additive multimodal ABI, so the D04
text ABI remains layout-compatible. A null input identifies a Source call, an
empty emission vector implements a Sink, and named emissions implement
multi-port output. Audio PCM, packed RGBA8 or I420 video, text, and bytes are copied
and validated by Rust before queue admission. See
`cpp/examples/multimodal_graph.cpp`.

## Agora RTC adapter

`Voxa::agora` is an optional C++17 adapter target. Its contract implementation
is always available; the real provider is enabled with `VOXA_ENABLE_AGORA=ON`
and `VOXA_AGORA_SDK_ROOT=/path/to/sdk`. It keeps Agora callbacks outside graph
execution and feeds owned frames through bounded external ingress. See
[`docs/providers/agora.md`](../providers/agora.md) for SDK compatibility,
build, credentials, and live acceptance steps.

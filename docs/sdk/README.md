# Voxa language SDKs / 多语言 SDK

Voxa now maintains the same minimum developer journey for Python, TypeScript,
and C++: install the package, define a Node, execute its lifecycle through a
bounded language domain, and verify the result from an independent consumer.

Voxa 现在为 Python、TypeScript 和 C++ 提供相同的最小完整链路：安装 SDK、
定义 Node、通过有界语言执行域运行生命周期，并在独立消费者项目中验收。

| SDK | Install/build | Define Node | Execute | Independent example | Current limitation |
| --- | --- | --- | --- | --- | --- |
| [Python](python.md) | maturin wheel / `pip install` | `TransformNode` / `GraphNodeFactory` | `NodeRunner` or Graph v1 | clean-venv examples | in-process, one in-flight callback |
| [TypeScript](typescript.md) | `@voxa/core` | `defineTransformNode` / `GraphNodeFactory` | Worker-hosted Graph v1 | packed strict `tsc` consumer | synchronous callbacks only |
| [C++](cpp.md) | CMake install package | derive `TransformNode` / `GraphNodeFactory` | `Runtime::run_graph` | external `find_package(Voxa)` project | text Transform factories in v1 |

## Honest runtime boundary / 当前边界

Python, TypeScript, and C++ hosts can now register trusted, exact-version text
Transform factories and execute them inside the same Graph v1 concurrent
Runtime as Rust built-ins. Graph JSON remains pure data and never loads code.
The host must supply implementations explicitly. D04 v1 intentionally accepts
empty foreign `node_config`, one text input, and one text output; general
schemas, media ports, sources, sinks, and package discovery remain follow-ups.

Python、TypeScript 与 C++ 宿主现在都能注册受信任的精确版本文本 Transform
Factory，并与 Rust 内置 Node 一起进入同一个 Graph v1 并发 Runtime。Graph
JSON 仍是纯数据，代码必须由宿主显式提供。D04 v1 暂时限定为空配置、单文本
输入和单文本输出；通用 Schema、媒体端口、Source、Sink 与包发现仍是后续工作。

## Acceptance gates / 验收门禁

```bash
./scripts/check-python.sh       # builds a wheel, installs it in a clean venv, runs examples
./scripts/check-node.sh         # packs npm tarball, installs, type-checks and runs TS consumer
./scripts/check-ffi.sh          # ABI tests plus installed CMake consumer when CMake exists
```

The bindings and native-ffi GitHub Actions run these gates on macOS and Linux.

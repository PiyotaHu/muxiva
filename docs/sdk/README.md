# Voxa language SDKs / 多语言 SDK

Voxa now maintains the same minimum developer journey for Python, TypeScript,
and C++: install the package, define a Node, execute its lifecycle through a
bounded language domain, and verify the result from an independent consumer.

Voxa 现在为 Python、TypeScript 和 C++ 提供相同的最小完整链路：安装 SDK、
定义 Node、通过有界语言执行域运行生命周期，并在独立消费者项目中验收。

| SDK | Install/build | Define Node | Execute | Independent example | Current limitation |
| --- | --- | --- | --- | --- | --- |
| [Python](python.md) | maturin wheel / `pip install` | `TransformNode` | `NodeRunner` | `examples/python` in a clean venv | in-process, one in-flight callback |
| [TypeScript](typescript.md) | `@voxa/core` | `defineTransformNode` | `NodeRunner` + Worker | strict `tsc` consumer package | synchronous callbacks only |
| [C++](cpp.md) | CMake install package | derive `TransformNode` | `Runtime::run_text` | external `find_package(Voxa)` project | focused single-node text runner |

## Honest runtime boundary / 当前边界

These SDKs execute Nodes through the foreign-language domains that Voxa
currently implements. General registration of Python, TypeScript, or C++ Node
factories into arbitrary Graph v1 JSON is not implemented yet. That work needs
a versioned factory/port/config contract in `voxa-core`; the SDKs do not pretend
that low-level registration metadata is an executable factory.

当前 SDK 已能开发并运行外语 Node，但尚不能把任意 Python、TypeScript 或 C++
Node Factory 注册到通用 Graph v1 JSON 后直接执行。后续需要在 `voxa-core` 中
增加版本化 Factory、Port 与配置契约，本次实现没有用表面 API 掩盖这一缺口。

## Acceptance gates / 验收门禁

```bash
./scripts/check-python.sh       # builds a wheel, installs it in a clean venv, runs examples
./scripts/check-node.sh         # packs npm tarball, installs, type-checks and runs TS consumer
./scripts/check-ffi.sh          # ABI tests plus installed CMake consumer when CMake exists
```

The bindings and native-ffi GitHub Actions run these gates on macOS and Linux.

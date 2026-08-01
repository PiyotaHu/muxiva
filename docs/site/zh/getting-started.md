# 安装与首次运行

Voxa 当前为 pre-alpha。仓库暂时提供源码安装；独立二进制、Python Wheel、npm
Package 与 Native SDK 压缩包的 Release Pipeline 仍在建设中。

## 环境要求

- Git；
- `rust-toolchain.toml` 固定的 Rust 工具链；
- Native 开发所需的 CMake 3.20+ 与 C11/C++17 编译器；
- Python 开发可选 CPython 与 maturin；
- TypeScript 开发可选 Node.js 与 pnpm。

## 一次安装 CLI

```bash
git clone https://github.com/PiyotaHu/Voxa.git
cd Voxa
cargo install --locked --path crates/voxa-cli
voxa --version
```

完成安装后，应用开发者日常只使用 `voxa`，不需要通过 `cargo run` 启动整个
Workspace。

## 运行分叉语音 Demo

```bash
voxa demo
```

默认会话运行 4 个 Turn。需要更长的可观察运行时，可使用
`voxa demo --turns 20 --interval-ms 1000`。

默认场景真实执行一张包含八个 Node、两处分叉和一次有状态汇合的语音图。只想
验证安装时可以运行：

```bash
voxa demo --scenario text
```

## 创建并运行 Graph

```bash
voxa init my-agent.voxa.json
voxa validate my-agent.voxa.json
voxa run my-agent.voxa.json
```

`validate` 不产生副作用。`run` 使用精确 Node Registry 编译 Graph、实例化
Factory，并通过并发 Runtime 在有界执行与关闭期限内运行。

## 打开 Studio

```bash
voxa studio my-agent.voxa.json
```

Studio 会在本机启动并生成随机访问 Token。下一步阅读
[Studio 指南](studio.md)。

## 真实语音门面 Demo

取得 Agora Native C++ SDK、三个短期 RTC Token 以及 DashScope 凭据后：

```bash
./examples/voice-agent/setup.sh /absolute/path/to/agora-native-sdk
./examples/voice-agent/run.sh
```

在 Studio 选择 Qwen Realtime 或 Cascade，填写 **Connections**，打开
**Voice Room**。详细凭据边界与验收步骤见仓库中的
`examples/voice-agent/README.md`。

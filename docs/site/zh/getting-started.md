# 安装与首次运行

Muxiva 当前为 pre-alpha。仓库暂时提供源码安装；独立二进制、Python Wheel、npm
Package 与 Native SDK 压缩包的 Release Pipeline 仍在建设中。

## 环境要求

- Git；
- `rust-toolchain.toml` 固定的 Rust 工具链；
- Native 开发所需的 CMake 3.20+ 与 C11/C++17 编译器；
- Python 开发可选 CPython 与 maturin；
- TypeScript 开发可选 Node.js 与 pnpm。

## 一次安装 CLI

```bash
git clone https://github.com/PiyotaHu/muxiva.git
cd Muxiva
cargo install --locked --path crates/muxiva-cli
muxiva --version
```

完成安装后，应用开发者日常只使用 `muxiva`，不需要通过 `cargo run` 启动整个
Workspace。

## CLI 入口

直接运行 `muxiva` 会显示三个推荐入口；`muxiva --help` 会解释每个命令：

| 命令 | 作用 |
| --- | --- |
| `muxiva studio [项目或图]` | 打开 Studio；省略参数时自动发现或创建工作区 |
| `muxiva init [目录]` | 创建包含 `graph.json` 与 `.muxiva/` 的完整项目 |
| `muxiva validate <项目或图>` | 只校验，不创建或执行 Node |
| `muxiva run <项目或图>` | 使用并发 Runtime 执行 Graph |
| `muxiva doctor [--voice]` | 检查工具链、项目和真实语音 Demo 就绪状态 |
| `muxiva simulate` | 运行无网络的合成 Runtime 测试夹具，不是产品 Demo |

## 第一次运行：真实语音助手

Muxiva 的开发者主体验是带真实凭据的 Qwen + Agora Voice Room，而不是合成 ASR、
LLM 或 TTS 输出。macOS 安装脚本会自动下载并校验 Agora SDK；Qwen 不需要下载
SDK。两个短期 RTC Token、API Key 与 Workspace ID 的申请步骤见
[从零运行真实语音 Agent](voice-demo.md)。

```bash
./examples/voice-agent/setup.sh
./examples/voice-agent/run.sh
```

## 创建并运行 Graph

```bash
muxiva init my-agent
muxiva validate my-agent
muxiva run my-agent
```

`init` 会创建 `my-agent/graph.json`、`.muxiva/nodes/`、`.muxiva/templates/` 与项目
README。`validate` 不产生副作用。`run` 使用精确 Node Registry 编译 Graph、实例化
Factory，并通过并发 Runtime 在有界执行与关闭期限内运行。传入单个 `.json` 文件的
旧用法仍然兼容。

## 打开 Studio

```bash
cd my-agent
muxiva studio
```

Studio 会自动发现 `graph.json`。如果当前目录不是项目，则安全创建
`muxiva.graph.json`；在 Muxiva 源码仓库根目录运行时，会自动打开旗舰 Voice Agent
工作区。服务只监听本机并生成随机访问 Token。下一步阅读
[Studio 指南](studio.md)。

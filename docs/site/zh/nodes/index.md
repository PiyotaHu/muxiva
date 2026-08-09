# 开发手册

Muxiva 面向两类开发者。第一类已经有自己的 Agent，需要把它接入实时音频、打断、
多模态数据流和可观测性；第二类需要开发 ASR、TTS、RTC、媒体处理等普通 Node。

这两类扩展都通过项目 Node Package 进入同一张类型化 Graph，但职责不同：

| 你的目标 | 应该从哪里开始 |
| --- | --- |
| 把现有 Agent 部署到 Muxiva | [Agent 集成](agent-integration.md) |
| 体验具有文件与编码能力的参考 Agent | [Pi 编码 Agent](pi-agent.md) |
| 编写 Python 算法或业务 Node | [Python](python.md) |
| 编写 TypeScript Agent / Web 业务 Node | [TypeScript](typescript.md) |
| 编写 Rust 高性能内置 Node | [Rust](rust.md) |
| 编写 C++ RTC、媒体或厂商 SDK Node | [C++](cpp.md) |

## 项目结构

一个典型的 Agent 项目如下：

```text
my-agent/
├── graph.json                         # 只描述 Node、Port、Edge 和配置
├── package.json                       # TypeScript Agent 依赖与锁文件
├── .env                               # 本地凭据，Git ignored
└── .muxiva/
    ├── nodes/my_agent/
    │   ├── muxiva.node.json           # Node 契约
    │   └── node.ts                    # Agent → Muxiva 的薄适配器
    ├── agents/my-agent/               # 独立 Agent 仓库的锁定版本
    └── workspaces/my-agent/            # Agent 获准读写的工作区
```

`graph.json` 不保存可执行代码。Node Package 的 `muxiva.node/v1` Manifest 声明发现与
校验信息；Agent 本体可以继续拥有自己的仓库、发布节奏、测试和权限策略。

## Node Manifest

| 字段 | 用途 |
| --- | --- |
| `package_id` | 文件系统安全的项目身份 |
| `display_name` | Studio Palette 中显示的名称 |
| `node_type` | 稳定 Factory 类型名 |
| `language` | `rust`、`cpp`、`python` 或 `typescript` |
| `factory_version` | 精确 Factory / 适配器版本 |
| `kind` | `source`、`transform` 或 `sink` |
| `category` | `transport`、`algorithm`、`media`、`control` 或 `utility` |
| `capability` | 稳定、可搜索的能力标识 |
| `entrypoint` | 语言实现入口 |
| `ports` | 名称、方向、Frame 类型和语义 Schema |
| `config_schema` | Node 配置的 JSON Schema |

## Studio-first 流程

普通 Node 可以直接在 Studio 中创建：

1. 为项目 Graph 打开 Studio；
2. 点击 **Create Node**；
3. 选择语言、角色、类型化 Port 与配置 Schema；
4. 编辑代码并点击 **Save & Register**；
5. 从 Palette 拖入 Graph，连接兼容 Port；
6. Validate、Run，并在 Observe 中检查调用、队列、消息和媒体。

已有 Agent 不建议把全部源码粘贴进 Studio。应将 Agent 保持在独立仓库，只在
`.muxiva/nodes/` 保存稳定适配器。完整 SOP 见 [Agent 集成](agent-integration.md)。

# Pi 编码 Agent 参考实现

Demo 2 现在使用独立仓库
[PiyotaHu/muxiva-pi-agent](https://github.com/PiyotaHu/muxiva-pi-agent)。它不是被复制进
Rust Core 的业务代码，而是模拟最终用户已经拥有、测试并发布自己的 Agent 后，再把
锁定版本部署到 Muxiva 项目的完整路径。

如果你要集成自己的 Agent，请先读 [Agent 集成 SOP](agent-integration.md)。本页只介绍
这份 Pi 参考实现。

## 代码放在哪里

| 内容 | 位置 | 归属 |
| --- | --- | --- |
| Pi 会话、Qwen 模型、Tool、文件权限 | `PiyotaHu/muxiva-pi-agent` | 独立 Agent 仓库 |
| 通用队列、Generation、取消适配 | `bindings/agent` 的 `@muxiva/agent` | Muxiva SDK |
| 两者装配 | `.muxiva/nodes/pi_agent/node.ts` | Demo 项目 |
| Port、配置、Connection Schema | `.muxiva/nodes/pi_agent/muxiva.node.json` | Demo 项目 |
| 拉取版本和依赖安装 | `examples/voice-agent/setup.sh` | Demo 部署 |

Demo 中的 `node.ts` 只有薄薄一层：

```typescript
import { defineAgentNode } from '@muxiva/agent'
import { createMuxivaPiAgentDriver } from '@piyotahu/muxiva-pi-agent'

export const PiAgentNode = defineAgentNode({
  createDriver: createMuxivaPiAgentDriver,
})
```

## Agent 能做什么

当前 `v0.2.1` 提供：

| Tool | 行为 |
| --- | --- |
| `workspace_info` | 查看工作区、权限和资源上限 |
| `list_files` | 列出目录，可选递归 |
| `read_file` | 读取 UTF-8 文件或行范围 |
| `search_files` | 在受限数量文件中搜索精确文本 |
| `write_file` | 创建文件；覆盖已有文件必须显式确认 |
| `replace_in_file` | 按预期匹配次数做精确代码替换 |
| `web_search` | 通过百炼执行真实联网搜索，返回综合答案、标题、站点和来源 URL |
| `get_current_time` | 查询指定时区当前时间 |
| `get_current_weather` | 通过 Open-Meteo 查询实时天气 |

这意味着 Demo 2 不再只是“会聊天”。你可以让它：

- “先列一下工作区都有哪些文件”；
- “读取需求，然后创建一个单页网站 `index.html`”；
- “把标题 Muxiva 改成 My Agent，其他内容不要动”；
- “搜索所有出现 TODO 的位置并告诉我行号”。
- “搜索今天发布的 Qwen 更新，引用来源并总结和当前模型的区别”。

文件会真实写入：

```text
examples/voice-agent/.muxiva/workspaces/pi-agent/
```

## 为什么默认没有 Shell

文件编辑能力与整机命令执行不是同一级别的权限。参考 Agent 默认提供足以完成网页和
代码文件任务的结构化 Tool，但没有 Shell、任意进程、任意删除或工作区外访问。

所有路径都相对 Graph 项目根目录解析。实现会检查路径穿越、符号链接逃逸、敏感文件
和资源上限；`.env`、`.env.*`、`.git`、`.ssh` 始终拒绝访问。

如果 Fork 后增加 Shell Tool，应把命令白名单、工作目录、超时、输出上限、网络策略和
人工确认作为 Agent 仓库自己的安全契约，而不是塞进 Muxiva Core。

## 安装发生了什么

```bash
./examples/voice-agent/setup.sh
```

脚本依次：

1. 从 GitHub 拉取 `PiyotaHu/muxiva-pi-agent` 的 `v0.2.1`；
2. 保存到被 Git 忽略的 `.muxiva/agents/muxiva-pi-agent`；
3. 使用项目 `package-lock.json` 安装 `@muxiva/agent`、外部 Agent 及 Pi 依赖；
4. 对 Demo 适配器与外部 Agent 分别运行 TypeScript 检查；
5. 运行外部 Agent 的文件权限测试；
6. 创建默认工作区，并继续构建 Qwen 与 Agora Node。

脚本会打印仓库、Tag、解析后的 Commit、工作区和权限。再次运行时，只有 Remote 一致、
工作区干净时才会复用；不会覆盖你在外部 Agent Checkout 中的修改。

## 配置自己的 Agent Fork

```bash
MUXIVA_PI_AGENT_REPOSITORY=https://github.com/your-org/your-agent.git \
MUXIVA_PI_AGENT_REF=v1.0.0 \
./examples/voice-agent/setup.sh
```

你的仓库需要保持同一个包导出 `createMuxivaPiAgentDriver`，或者同步修改 Demo 的薄适配器
导入名。更普遍的自有 Agent 接入方式见 [Agent 集成](agent-integration.md)。

## 运行和验收

```bash
muxiva doctor --voice
./examples/voice-agent/run.sh --studio
```

在 Studio 选择 **Pi Agent Full-Duplex Cascade（Demo 2）**，进入 Voice Room。先说：

> 在工作区创建一个有渐变背景、显示当前时间的 `index.html`。

完成后检查真实文件：

```bash
ls -la examples/voice-agent/.muxiva/workspaces/pi-agent
```

打开 Observe：Tool 生命周期应出现在 Semantic Trace；`pi-agent` 的 Text 输出进入 TTS；
用户插话产生的 Signal 会取消当前 Pi Turn，旧 Generation 的晚到输出不会继续播报。

再问一个必须联网的问题，例如“搜索今天的 Qwen 新闻并给出来源”。Observe 中应看到
`web_search` 的 `tool.started/completed`；Tool 详情包含 `duration_ms`、`search_strategy`、
`search_calls` 和结构化 `sources`。它复用 Connections 中的百炼 Key 和 Workspace ID，
无需增加凭据，但搜索调用会按百炼规则计费。

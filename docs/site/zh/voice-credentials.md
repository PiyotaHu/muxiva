# 语音 Demo 凭据：从申请到填入 Muxiva

这是一份可以逐项照做的首次运行清单。最终你需要自己取得 **5 个值**：Agora App ID、
两个 RTC Token、百炼 API Key、百炼 Workspace ID。Channel 和两个 UID 使用 Muxiva 的
预设值即可。

!!! warning "先完成项目级 `.env`"
    Headless Runtime 不依赖 Studio。先把凭据写入当前仓库副本的
    `examples/voice-agent/.env`，再运行 `muxiva doctor --voice`；Studio Connections
    只是编辑同一个文件的可选图形界面。

## 一眼看懂：什么填到哪里

| 服务 | Muxiva 字段 | 首次运行填什么 |
| --- | --- | --- |
| Agora | App ID | Agora 项目的 32 位 App ID |
| Agora | Channel | `muxiva-demo` |
| Agora | Browser UID | `1001` |
| Agora | Browser Token | 为 Channel `muxiva-demo`、UID `1001` 生成的 RTC Token |
| Agora | Muxiva Bot UID | `2001` |
| Agora | Muxiva Bot Token | 为 Channel `muxiva-demo`、UID `2001` 生成的 RTC Token |
| 百炼 | API Key | 华北 2（北京）业务空间创建的按量付费 API Key |
| 百炼 | Workspace ID | 上述 API Key 所属业务空间的 ID |

Agora **App Certificate 不填入 Muxiva**。它只在 Token Builder 中生成 Token 时使用。

## A. 申请 Agora App ID 与两个 Token

### A1. 创建 Agora 项目

1. 打开 [Agora Console](https://console.agora.io/) 并注册或登录。
2. 进入 [Projects](https://console.agora.io/legacy/project-management)，点击 **Create New**。
3. 填写项目名称；Authentication mechanism 选择
   **Secured mode: APP ID + Token (Recommended)**。
4. 创建完成后，在项目列表的 **App ID** 列点击复制。这就是 `.env` 的
   `MUXIVA_AGORA_APP_ID`。

Agora 官方的[账号和项目指南](https://docs.agora.io/en/realtime-media/voice/manage-agora-account)
也给出了相同流程。

### A2. 找到 App Certificate

1. 在 Projects 页面找到刚才的项目，点击铅笔图标。
2. 在 Security 区域找到 **Primary Certificate**，点击复制。
3. 临时保存它，下一步要用；不要放进 `.env`、Graph、网页或 Git。

### A3. 在 Token Builder 连续生成两次

打开 [Agora Token Builder](https://agora-token-generator-demo.vercel.app/)。如果页面要求选择
产品，选择 **RTC**。两次生成都填写同一个 App ID、App Certificate 和 Channel；只有 UID
不同。

| Token Builder 输入框 | 第一次 | 第二次 |
| --- | --- | --- |
| App ID | 你的 App ID | 同一个 App ID |
| App Certificate | Primary Certificate | 同一个 Certificate |
| User ID / UID | `1001` | `2001` |
| Token expiration time | `3600`（首次测试一小时） | `3600` |
| Channel name | `muxiva-demo` | `muxiva-demo` |
| 生成结果填入 `.env` | `MUXIVA_AGORA_WEB_TOKEN` | `MUXIVA_AGORA_BOT_TOKEN` |

!!! important "Channel 不需要提前创建"
    `muxiva-demo` 只是双方约定的房间名。它区分大小写；Token Builder、Studio 和所有客户端
    必须逐字符一致。两个 Token 与 UID 绑定，不能交换使用。

Agora Console 也提供 **Generate Temp Token**。为了让 Muxiva 的浏览器与 Bot 使用明确且
不同的数字 UID，首次运行推荐使用上面的 Token Builder 分别生成两个 UID Token。

## B. 申请阿里云百炼 API Key 与 Workspace ID

### B1. 开通服务并固定地域

1. 登录[阿里云百炼控制台](https://bailian.console.aliyun.com/)。
2. 如果提示未开通或未实名认证，先按页面提示完成。
3. 在页面右上角将地域切换为 **华北 2（北京）**。Muxiva 当前 Qwen Node 使用北京
   Workspace 专属端点，之后不要切换地域。

### B2. 创建 API Key

1. 在百炼控制台进入 **API Key** 页面，点击 **创建 API Key**。
2. “归属业务空间”第一次建议选择**默认业务空间**；权限选择“全部”。
3. 创建后立即复制完整 Key。关闭弹窗后通常不能再次查看明文；丢失时应重置或新建。
4. 将它填入 `.env` 的 `DASHSCOPE_API_KEY`。

官方步骤：[如何获取 API Key](https://help.aliyun.com/zh/model-studio/get-api-key/)。Muxiva
需要的是百炼按量付费 API Key，不是 Coding Plan 或 Token Plan 的专用 Key。

### B3. 复制同一业务空间的 Workspace ID

1. 保持地域为 **华北 2（北京）**。
2. 点击控制台右上角的业务空间入口，在当前空间信息中复制 **Workspace ID**；也可进入
   “业务空间管理”，从 Workspace ID 列复制。
3. 确认这个 Workspace 正是上一步 API Key 的“归属业务空间”。
4. 将它填入 `.env` 的 `DASHSCOPE_WORKSPACE_ID`。

官方步骤：[获取 Workspace ID](https://help.aliyun.com/zh/model-studio/obtain-the-app-id-and-workspace-id)。
API Key 和 Workspace ID 若跨地域或跨业务空间组合，WebSocket 会鉴权失败。

这里没有“下载 Qwen SDK”步骤。Muxiva 的 Python Node 直接调用百炼官方 WebSocket/HTTP
协议，`setup.sh` 会安装所需 Python 依赖。

## C. 在 Muxiva 中只填写一次

```bash
cd /你的路径/Muxiva
cp examples/voice-agent/.env.example examples/voice-agent/.env
# 使用文本编辑器填写下面列出的值
# macOS 默认打开 Studio
./examples/voice-agent/run.sh
# Linux / Docker / 服务器使用 Headless Runtime
./examples/voice-agent/run.sh --headless
```

1. 将 Agora 与百炼字段保存到项目 `.env`。
2. 运行 `muxiva doctor --voice`，确认没有 `MISSING`。
3. macOS/Windows 本地开发运行 `run.sh`（或显式 `--studio`），在浏览器进入 Studio。
4. Linux/部署环境运行 `run.sh --headless`，等待 `runtime.started mode=headless`。
5. Headless 模式另开终端执行 `cd examples/voice-agent && npm run voice-room`。
6. 打开 `http://127.0.0.1:4173`，测试 Backend URL 后开始通话。
7. 允许麦克风权限，说一句完整的话并停顿约一秒。

凭据只写入 `examples/voice-agent/.env`，并已被 Git 忽略；下次启动会自动读取，
不需要重复填写。文件形状如下，值不要提交：

`.env` 是**当前项目副本本地**的配置。新 clone、另一台机器或另一个工作目录不会自动
共享它；请显式复制旧项目的 `.env`，或从 `.env.example` 新建一次。Muxiva 遇到缺失项时
会在创建任何 Node Host 前列出字段和它实际读取的绝对路径。

```dotenv
MUXIVA_AGORA_APP_ID="..."
MUXIVA_AGORA_CHANNEL="muxiva-demo"
MUXIVA_AGORA_WEB_UID="1001"
MUXIVA_AGORA_WEB_TOKEN="..."
MUXIVA_AGORA_BOT_UID="2001"
MUXIVA_AGORA_BOT_TOKEN="..."
DASHSCOPE_API_KEY="..."
DASHSCOPE_WORKSPACE_ID="..."
```

## D. 如何确认配置和定位失败

```bash
muxiva doctor --voice
tail -f examples/voice-agent/.muxiva/runtime.log
```

`doctor` 只检查工具链、官方 Node 和凭据是否存在，不会替你创建 Token，也不会输出密钥。
真实会话按以下顺序定位：

1. Voice Room：`Browser joined Agora`、`microphone published`；
2. 日志：`[MUXIVA][AGORA][participant.joined] uid=1001`；
3. 日志：`[MUXIVA][AGORA][audio.received]`；
4. 日志：`[MUXIVA][QWEN][event] type=input_audio_buffer.speech_started`；
5. 日志：`response.created`、`[MUXIVA][AGORA][data.published]` 和音频输出开始增长；
6. Voice Room 的 Client Messages 持续增长，左右两侧聊天消息正常显示。

| 现象 | 优先检查 |
| --- | --- |
| Agora 加入失败 | App ID、Channel、UID 是否与各自 Token 完全一致；Token 是否过期 |
| 只有 Browser 或 Bot 一侧加入 | 是否把 `1001` 与 `2001` 的 Token 填反 |
| Agora 有输入但 Qwen 无事件 | 北京地域、Key 与 Workspace 是否同一业务空间 |
| 能连接但不回复 | 完整说话后停顿；查看 Qwen `speech_started/stopped` 与 `response.created` |
| 听到自己的声音 | 关闭历史 Voice Room 标签页；使用耳机；确认页面显示 AEC 已开启 |

继续：[完整安装与运行流程](voice-demo.md)。

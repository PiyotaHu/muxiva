# 语音 Demo 凭据：从申请到填入 Voxa

这是一份可以逐项照做的首次运行清单。最终你需要自己取得 **5 个值**：Agora App ID、
两个 RTC Token、百炼 API Key、百炼 Workspace ID。Channel 和两个 UID 使用 Voxa 的
预设值即可。

!!! warning "先别点击 Run"
    先让 Studio **Connections** 中的 Agora RTC 与 Alibaba Cloud Model Studio 两张卡片
    都显示 **Ready**，再选择图并进入 Voice Room。

## 一眼看懂：什么填到哪里

| 服务 | Voxa 字段 | 首次运行填什么 |
| --- | --- | --- |
| Agora | App ID | Agora 项目的 32 位 App ID |
| Agora | Channel | `voxa-demo` |
| Agora | Browser UID | `1001` |
| Agora | Browser Token | 为 Channel `voxa-demo`、UID `1001` 生成的 RTC Token |
| Agora | Voxa Bot UID | `2001` |
| Agora | Voxa Bot Token | 为 Channel `voxa-demo`、UID `2001` 生成的 RTC Token |
| 百炼 | API Key | 华北 2（北京）业务空间创建的按量付费 API Key |
| 百炼 | Workspace ID | 上述 API Key 所属业务空间的 ID |

Agora **App Certificate 不填入 Voxa**。它只在 Token Builder 中生成 Token 时使用。

## A. 申请 Agora App ID 与两个 Token

### A1. 创建 Agora 项目

1. 打开 [Agora Console](https://console.agora.io/) 并注册或登录。
2. 进入 [Projects](https://console.agora.io/legacy/project-management)，点击 **Create New**。
3. 填写项目名称；Authentication mechanism 选择
   **Secured mode: APP ID + Token (Recommended)**。
4. 创建完成后，在项目列表的 **App ID** 列点击复制。这就是 Studio 的 **App ID**。

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
| Channel name | `voxa-demo` | `voxa-demo` |
| 生成结果填入 Studio | Browser Token | Voxa Bot Token |

!!! important "Channel 不需要提前创建"
    `voxa-demo` 只是双方约定的房间名。它区分大小写；Token Builder、Studio 和所有客户端
    必须逐字符一致。两个 Token 与 UID 绑定，不能交换使用。

Agora Console 也提供 **Generate Temp Token**。为了让 Voxa 的浏览器与 Bot 使用明确且
不同的数字 UID，首次运行推荐使用上面的 Token Builder 分别生成两个 UID Token。

## B. 申请阿里云百炼 API Key 与 Workspace ID

### B1. 开通服务并固定地域

1. 登录[阿里云百炼控制台](https://bailian.console.aliyun.com/)。
2. 如果提示未开通或未实名认证，先按页面提示完成。
3. 在页面右上角将地域切换为 **华北 2（北京）**。Voxa 当前 Qwen Node 使用北京
   Workspace 专属端点，之后不要切换地域。

### B2. 创建 API Key

1. 在百炼控制台进入 **API Key** 页面，点击 **创建 API Key**。
2. “归属业务空间”第一次建议选择**默认业务空间**；权限选择“全部”。
3. 创建后立即复制完整 Key。关闭弹窗后通常不能再次查看明文；丢失时应重置或新建。
4. 将它填入 Studio 的 **Alibaba Cloud Model Studio → API Key**。

官方步骤：[如何获取 API Key](https://help.aliyun.com/zh/model-studio/get-api-key/)。Voxa
需要的是百炼按量付费 API Key，不是 Coding Plan 或 Token Plan 的专用 Key。

### B3. 复制同一业务空间的 Workspace ID

1. 保持地域为 **华北 2（北京）**。
2. 点击控制台右上角的业务空间入口，在当前空间信息中复制 **Workspace ID**；也可进入
   “业务空间管理”，从 Workspace ID 列复制。
3. 确认这个 Workspace 正是上一步 API Key 的“归属业务空间”。
4. 将它填入 Studio 的 **Workspace ID**。

官方步骤：[获取 Workspace ID](https://help.aliyun.com/zh/model-studio/obtain-the-app-id-and-workspace-id)。
API Key 和 Workspace ID 若跨地域或跨业务空间组合，WebSocket 会鉴权失败。

这里没有“下载 Qwen SDK”步骤。Voxa 的 Python Node 直接调用百炼官方 WebSocket/HTTP
协议，`setup.sh` 会安装所需 Python 依赖。

## C. 在 Voxa 中只填写一次

```bash
cd /你的路径/Voxa
./examples/voice-agent/run.sh
```

1. Studio 打开后，点击顶部 **Connections**。
2. 填完两张卡片，点击 **Save connections**。
3. 两张卡片都显示 **Ready** 后，点击 **Templates → Qwen Realtime**。
4. 点击 **Run**，等待 Studio 显示 Runtime 已运行。
5. 点击 **Voice Room → Start live conversation**，允许麦克风权限。
6. 说一句完整的话，然后停顿约一秒，等待首次回复。

保存后，凭据写入 `examples/voice-agent/.env`，权限为 `0600`，并已被 Git 忽略；下次
启动会自动读取，不需要重复填写。文件形状如下，值不要提交：

```dotenv
VOXA_AGORA_APP_ID="..."
VOXA_AGORA_CHANNEL="voxa-demo"
VOXA_AGORA_WEB_UID="1001"
VOXA_AGORA_WEB_TOKEN="..."
VOXA_AGORA_BOT_UID="2001"
VOXA_AGORA_BOT_TOKEN="..."
DASHSCOPE_API_KEY="..."
DASHSCOPE_WORKSPACE_ID="..."
```

## D. 如何确认配置和定位失败

```bash
voxa doctor --voice
tail -f examples/voice-agent/.voxa/runtime.log
```

`doctor` 只检查工具链、官方 Node 和凭据是否存在，不会替你创建 Token，也不会输出密钥。
真实会话按以下顺序定位：

1. Voice Room：`Browser joined Agora`、`microphone published`；
2. 日志：`[VOXA][AGORA][participant.joined] uid=1001`；
3. 日志：`[VOXA][AGORA][audio.received]`；
4. 日志：`[VOXA][QWEN][event] type=input_audio_buffer.speech_started`；
5. 日志：`response.created`、`[VOXA][AGORA][data.published]` 和音频输出开始增长；
6. Voice Room 的 Client Messages 持续增长，左右两侧聊天消息正常显示。

| 现象 | 优先检查 |
| --- | --- |
| Agora 加入失败 | App ID、Channel、UID 是否与各自 Token 完全一致；Token 是否过期 |
| 只有 Browser 或 Bot 一侧加入 | 是否把 `1001` 与 `2001` 的 Token 填反 |
| Agora 有输入但 Qwen 无事件 | 北京地域、Key 与 Workspace 是否同一业务空间 |
| 能连接但不回复 | 完整说话后停顿；查看 Qwen `speech_started/stopped` 与 `response.created` |
| 听到自己的声音 | 关闭历史 Voice Room 标签页；使用耳机；确认页面显示 AEC 已开启 |

继续：[完整安装与运行流程](voice-demo.md)。

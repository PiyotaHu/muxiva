# 从零运行真实语音 Agent

本页从一个干净的 macOS 开发环境开始，不假设你了解 Agora 或 Qwen。完成后，浏览器
麦克风通过 Agora 进入 Voxa Graph，由 Qwen 生成实时回复，再通过 Agora 播放出来。

!!! danger "只有 App ID 还不能运行"
    必须先准备 2 个 Agora RTC Token、百炼 API Key 和 Workspace ID。没有全部配置时，
    不要点击 Run 或 Voice Room。请先逐项完成[语音凭据配置清单](voice-credentials.md)。

!!! info "你真正需要准备的东西"
    Agora 需要一个账号、App ID 和两个临时 RTC Token。Qwen **不需要下载 SDK**，
    只需要阿里云百炼 API Key 与 Workspace ID。Voxa 会自动下载并校验 Agora macOS
    SDK，也会在隔离的 Python 环境中安装 Qwen WebSocket 依赖。

## 0. 当前支持范围

- 一键安装路径已在 Apple Silicon macOS 上验证；Agora macOS SDK 固定为 `4.6.2`。
- Qwen Provider 当前使用阿里云百炼**华北 2（北京）**端点，API Key 和 Workspace
  必须来自同一区域。
- Windows/其他平台目前需要从 [Agora SDK 官方页面](https://docs.agora.io/en/api-reference/sdks?product=voice)
  手动下载，并把解压目录传给 `setup.sh`。

## 1. 安装 Voxa 与 Provider

先准备 Git、Rust、Python 3、CMake 3.20+ 与 Xcode Command Line Tools，然后运行：

```bash
git clone https://github.com/PiyotaHu/Voxa.git
cd Voxa
cargo install --locked --path crates/voxa-cli
./examples/voice-agent/setup.sh
```

最后一条命令会：

1. 从 [Agora 官方 macOS SDK 仓库](https://github.com/AgoraIO/AgoraRtcEngine_macOS/tree/4.6.2)
   对应的官方 CDN 下载 RTC Basic 所需 XCFramework；
2. 对每个压缩包执行 SHA-256 校验；
3. 创建 `examples/voice-agent/.voxa/venv` 并安装 `websocket-client`；
4. 编译 `agora_audio_source` 与 `agora_audio_sink` C++ Node Pack。

出现以下三行才代表安装完成：

```text
[VOXA][READY] Native and Python Node Packs are installed.
[VOXA][AGORA] sdk=.../build/vendor/agora-macos-4.6.2
[VOXA][QWEN]  python=.../.voxa/venv/bin/python (no Qwen SDK download required)
```

如果你已经手动下载 SDK，也可以运行：

```bash
./examples/voice-agent/setup.sh /你的/Agora-SDK-解压目录
```

## 2. 申请 Agora App ID 与 Token

如果这是第一次申请，请直接按[Agora App ID、Certificate、Token Builder 每个输入框的逐项指南](voice-credentials.md#a-agora-app-id-token)
操作。下面只是完成标准的摘要。

1. 打开 [Agora Console](https://console.agora.io/) 并注册或登录。
2. 进入 [Projects](https://console.agora.io/legacy/project-management)，点击
   **Create New**，认证方式选择 **Secured mode: APP ID + Token**。
3. 复制项目的 **App ID**。
4. 选一个 Channel 名称，例如 `voxa-demo`。后面所有 Token 必须使用完全相同的名称。
5. 按 Agora 官方的[账号与临时 Token 指南](https://docs.agora.io/en/realtime-media/voice/manage-agora-account)
   打开项目安全配置或 [Agora Token Builder](https://agora-token-generator-demo.vercel.app/)，
   为同一个 Channel 生成两个短期 RTC Token：

| Studio 字段 | UID | 第一次运行建议角色 | 用途 |
| --- | ---: | --- | --- |
| Browser UID / Token | `1001` | Publisher | 浏览器采集麦克风并播放音频 |
| Voxa Bot UID / Token | `2001` | Publisher | 同一个 C++ RTC Engine 接收麦克风并发布助手音频 |

!!! warning "不要暴露 App Certificate"
    App Certificate 只用于服务端生成 Token。不要把它填进 Studio、网页或提交到 Git。
    临时 Token 适合本地体验；生产环境必须部署自己的 Token Server。

## 3. 申请 Qwen 凭据

如果不熟悉百炼地域与业务空间，请直接按[百炼 API Key 与 Workspace ID 逐项指南](voice-credentials.md#b-api-key-workspace-id)
操作。Key 与 Workspace ID 必须是华北 2（北京）同一业务空间的一对值。

1. 打开[阿里云百炼控制台](https://bailian.console.aliyun.com/)，选择
   **华北 2（北京）**并开通服务。
2. 按官方[获取 API Key](https://help.aliyun.com/zh/model-studio/get-api-key)指南创建 Key，
   创建成功后立即保存明文。
3. 按官方[首次调用 Qwen](https://help.aliyun.com/zh/model-studio/first-api-call-to-qwen)
   指南找到同一 Workspace 的 **Workspace ID**。

这里没有“Qwen SDK 下载”步骤。Voxa 的 Python Provider 直接使用官方 WebSocket/HTTP
协议，`setup.sh` 已安装唯一的第三方 Python 依赖。Realtime 图默认使用
`qwen-audio-3.0-realtime-flash`；级联图使用 Qwen ASR、LLM 与 TTS。

## 4. 启动并填写 Studio

```bash
voxa doctor --voice
./examples/voice-agent/run.sh
```

`doctor` 应显示两个 Agora Node Pack 为 `mode=agora-native`，并显示
`qwen-python dependency=websocket`。凭据未配置时它会逐项打印 `MISSING`，这是诊断结果，
不是可以跳过的提示。然后在 Studio 中：

1. 打开 **Connections**。
2. 在 **Alibaba Cloud Model Studio** 填写 API Key、Workspace ID。
3. 在 **Agora RTC** 填写 App ID、Channel，以及 `1001`、`2001` 对应的 UID/Token。
4. 点击 **Save connections**，确认两张卡片都显示 **Ready**；否则 Runtime 不会启动。
5. 保存后进入 **Templates**，第一次选择 **Qwen Realtime**。
6. 打开 **Voice Room**，点击 **Start live conversation**，允许麦克风权限。
7. 自然说话；助手播放时再次开口，验证全双工打断。

点击 Save connections 后，值会保存到 `examples/voice-agent/.env`（权限 `0600`、Git
忽略）。以后再次运行无需重复填写。也可以参考 `.env.example` 手动创建该文件。

Realtime 跑通后，再切换 **Qwen Cascade**，观察 VAD → ASR → LLM → TTS 的各阶段。
会话会持续运行，直到点击 **End session**。

## 运行日志与链路定位

`run.sh` 会同时把终端输出保存到 `examples/voice-agent/.voxa/runtime.log`。遇到“已经
连接但没有回复”时，按下面的顺序找第一个没有增长的指标：

1. Voice Room 显示浏览器已加入、麦克风已发布；
2. 日志出现 `[VOXA][AGORA][participant.joined] uid=1001`；
3. 日志出现 `[VOXA][AGORA][audio.received]`，Studio 的 `agora-in.audio_out` 增长；
4. `audio-to-qwen`、Qwen Node 调用与字幕开始增长；
5. `qwen-audio` 和 `agora-out` 增长，浏览器听到回复。

第一个没有出现的步骤，就是故障所在层。凭据值不会写入日志。

## 5. 常见错误

| 现象 | 原因与处理 |
| --- | --- |
| `Agora SDK directory does not exist` | 路径不是解压目录；macOS 直接重新运行无参数 `setup.sh` |
| `AgoraRtcKit.xcframework` not found | 手动下载了错误平台或不完整包；使用一键下载命令 |
| `qwen-python ready=false` | 没运行 `setup.sh`，或项目虚拟环境损坏；重新运行安装 |
| Qwen 返回鉴权/模型错误 | API Key、Workspace ID、模型必须属于华北 2（北京）同一 Workspace |
| Agora 加入 Channel 失败 | App ID、Channel、UID 必须与生成该 Token 时完全一致，Token 也不能过期 |
| 页面没有麦克风 | 浏览器未授权；在浏览器站点权限中允许本地 Studio 使用麦克风 |

## 6. 工程验收

不使用凭据时，可以验证代码、Provider 边界与动态 ABI：

```bash
./scripts/check-provider-boundaries.sh
./scripts/check-voice-node-packs.sh
```

它们不会伪装成真实通话。完整验收标准是：真实加入 Agora Channel、真实麦克风输入、
Qwen 返回字幕和语音，并且在播放期间成功插话打断。

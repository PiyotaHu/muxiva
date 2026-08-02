# 语音 Demo 凭据：逐字段配置

如果你现在只有 Agora App ID，连同 Voxa 预设的 Channel，完成度是 **2/6**。先不要点击 Studio 的 **Run**
或 **Voice Room**。按照本页取得其余 4 个值，直到 Connections 中两张卡片都显示
**Ready**。

!!! warning "不要把密钥发到 Issue、聊天或 Git"
    App ID 不是密钥，但 App Certificate、RTC Token 和 Qwen API Key 都应保密。
    本地临时 Token 仅用于首次体验，生产环境必须使用服务端 Token 服务。

## 最终需要填写什么

打开 `./run.sh` 启动的 Studio，点击顶部 **Connections**。界面共有两张卡片：

### Agora RTC

| Studio 字段 | 第一次运行填什么 | 从哪里获得 |
| --- | --- | --- |
| App ID | 你的 32 位 Agora App ID | Agora Console 的 Projects 页面 |
| Channel | 保持 `voxa-demo` | 这是你自己选择的频道名 |
| Browser UID | 保持 `1001` | Voxa 预设 |
| Browser Token | App ID + `voxa-demo` + UID `1001` 生成的 RTC Token | Agora Token Builder |
| Voxa Bot UID | 保持 `2001` | Voxa 预设 |
| Voxa Bot Token | App ID + `voxa-demo` + UID `2001` 生成的 RTC Token | Agora Token Builder |

### Alibaba Cloud Model Studio

| Studio 字段 | 填什么 | 从哪里获得 |
| --- | --- | --- |
| API Key | 华北 2（北京）创建的百炼 API Key | 百炼控制台 API Key 页面 |
| Workspace ID | 上述 Key 所属业务空间的 ID | 百炼控制台右上角业务空间菜单 |

## 第一步：生成两个 Agora Token

### 1. 找到 App Certificate

1. 打开 [Agora Console](https://console.agora.io/)；
2. 进入 **Projects**，找到 App ID 所属项目；
3. 点击编辑图标，在 Security 中复制 **Primary Certificate**。

App Certificate 只用于生成 Token，**不要填进 Studio**。

### 2. 在 Token Builder 生成两次

打开 [Agora Token Builder](https://agora-token-generator-demo.vercel.app/)，选择 RTC，
每次都使用同一个 App ID、App Certificate 和 Channel `voxa-demo`：

| 第几次 | UID | Token 填入 Studio 的位置 |
| ---: | ---: | --- |
| 1 | `1001` | Browser Token |
| 2 | `2001` | Voxa Bot Token |

第一次体验可为两者选择 Publisher 权限和足够完成测试的短期有效期。UID 必须使用
**数字 UID**，Token 不能互换。Agora 官方也说明，临时 Token 由项目安全页面或 Token
Builder 生成；参见 [Agora 账号与临时 Token 官方指南](https://docs.agora.io/en/realtime-media/voice/manage-agora-account)。

## 第二步：取得 Qwen 的两个值

1. 打开[阿里云百炼控制台](https://bailian.console.aliyun.com/)，右上角选择
   **华北 2（北京）**；
2. 进入 API Key 页面，创建按量付费 API Key，创建后立即复制；
3. 打开右上角业务空间菜单，复制这个 Key 所属空间的 **Workspace ID**；
4. 保证两个值来自同一地域、同一业务空间。

官方入口：[获取 API Key](https://help.aliyun.com/zh/model-studio/get-api-key/) ·
[获取 Workspace ID](https://help.aliyun.com/zh/model-studio/obtain-the-app-id-and-workspace-id)。

## 第三步：填写、确认、运行

```bash
cd examples/voice-agent
./run.sh
```

1. Studio 打开后点击 **Connections**；
2. 按上面的表填写，点击 **Save connections**；
3. 确认 Agora RTC 和 Alibaba Cloud Model Studio 两张卡片都显示 **Ready**；
4. 点击 **Templates**，第一次选择 **Qwen Realtime**；
5. 点击 **Voice Room**，再点击 **Start live conversation**；
6. 允许浏览器使用麦克风，然后开始说话。

Connections 会把值写入语音项目根目录的 `.env`，文件权限为 `0600` 且已被 Git
忽略。只需填写一次，关闭并重新启动 Studio 后会自动加载；凭据不会写进 Graph。

## `doctor --voice` 到底检查什么

`voxa doctor --voice` 是环境诊断，不负责申请凭据：

- `native-node-pack PASS`：Agora C++ Node Pack 已正确编译；
- `qwen-python PASS`：Python WebSocket 依赖可用；
- `voice-credentials WARN/MISSING`：当前 Shell 或项目 `.env` 还缺哪些值；
- `--strict`：任何缺失都会让命令返回非零，适合 CI。

Studio 保存后，单独运行的 doctor 也能从项目 `.env` 看到配置状态，但不会输出值。
启动 Graph 时仍会检查所需凭据；缺失时只打开 Connections，不会启动 C++ Node。

继续：[完整安装与运行流程](voice-demo.md)。

# Headless Runtime 与独立网页

Muxiva 的部署边界是：**Linux 服务器运行 Graph，用户设备运行网页**。Studio 是可选的
Graph 设计与本地观测工具，不是生产 Runtime，也不是 Voice Room 的 Web Server。

```text
Linux / Docker                         macOS / Windows / phone browser
┌──────────────────────────┐           ┌──────────────────────────────┐
│ muxiva serve graph.json  │           │ Standalone Voice Room        │
│ Rust Runtime + Nodes     │◄─Agora───►│ microphone / speaker / chat  │
│ :8080 Bootstrap API only │◄──HTTPS───│ one-time RTC configuration   │
└──────────────────────────┘           └──────────────────────────────┘
```

浏览器音频和消息直接走 Agora RTC。HTTP API 只在加入频道前返回浏览器身份所需的 App ID、
Channel、Web UID 和短期 Web Token；它不代理音频，不提供 Graph 编辑，也不会返回
DashScope Key、Agora Bot Token 或 App Certificate。

## CLI 语义

| 命令 | 用途 | 是否依赖 Studio |
| --- | --- | --- |
| `muxiva validate graph.json` | 只编译、校验 Graph 和 Node Library | 否 |
| `muxiva run graph.json` | 运行会自然结束的批处理 Graph，默认最长 30 秒 | 否 |
| `muxiva serve graph.json` | 长期运行实时 Graph，并提供最小 Bootstrap API | 否 |
| `muxiva studio graph.json` | 可视化设计、单机调试和 Observe | 是，它本身就是 Studio |

Linux 上启动真实语音 Graph：

```bash
cd muxiva
./examples/voice-agent/setup.sh /absolute/path/to/agora-linux-sdk
cp examples/voice-agent/.env.example examples/voice-agent/.env
# 编辑 .env，填入 Agora 和百炼凭据
muxiva doctor --voice
muxiva serve examples/voice-agent/graph.json
```

也可以用示例脚本显式选择 Headless；Linux 即使不写参数也默认该模式：

```bash
./examples/voice-agent/run.sh --headless
```

成功后应看到：

```text
[MUXIVA][INFO][runtime.started] mode=headless ...
[MUXIVA][INFO][client-api.ready] base_url=http://127.0.0.1:8080 ...
[MUXIVA][INFO][runtime.control] stop=Ctrl-C studio=not-required
```

`Ctrl-C` 或容器的 `SIGTERM` 会触发有界关闭。日志仍写入
`examples/voice-agent/.muxiva/runtime.log`。

## 启动独立 Voice Room

网页源码位于 `examples/voice-agent/web/`，不在 `.muxiva` 中，也不由 Studio 提供。
它是纯静态页面；随附的 Node 脚本只负责提供 HTML/CSS/JS，不运行 Graph。

### macOS

```bash
cd examples/voice-agent
npm run voice-room
open http://127.0.0.1:4173
```

### Windows PowerShell

```powershell
cd examples\voice-agent
npm run voice-room
Start-Process http://127.0.0.1:4173
```

### Linux 桌面

```bash
cd examples/voice-agent
npm run voice-room
xdg-open http://127.0.0.1:4173
```

页面中填写 `muxiva serve` 打印的 **Backend URL**，然后点 **Test connection**。本机默认是
`http://127.0.0.1:8080`。测试通过后再点 **Start live conversation**。

如果不想安装 Node，也可使用 Docker 托管同一份静态文件：

```bash
docker run --rm -p 4173:80 \
  --mount type=bind,source="$PWD/examples/voice-agent/web",target=/usr/share/nginx/html,readonly \
  nginx:alpine
```

## 从本机连接无 GUI Linux

### 推荐：SSH 隧道

服务器仍只监听回环地址：

```bash
# Linux server
muxiva serve examples/voice-agent/graph.json
```

在 macOS/Windows 开发机建立隧道：

```bash
ssh -L 8080:127.0.0.1:8080 user@linux-host
```

随后在开发机启动 Voice Room，Backend URL 仍填 `http://127.0.0.1:8080`。这种方式不用
开放 8080 端口，也不需要 Client API Token。

### 公网地址或 Docker 端口映射

先在服务器项目 `.env` 生成并保存独立访问令牌：

```bash
python3 -c 'import secrets; print(secrets.token_urlsafe(32))'
# 将输出填到 examples/voice-agent/.env：
# MUXIVA_CLIENT_API_TOKEN="..."

muxiva serve examples/voice-agent/graph.json \
  --host 0.0.0.0 \
  --port 8080 \
  --allow-origin http://127.0.0.1:4173
```

页面 Backend URL 填 `http://你的服务器IP:8080`，Client API token 填同一个值。Muxiva 在
非回环地址没有 Token 时会拒绝启动；`--allow-origin '*'` 也会被拒绝。

生产环境应把 Voice Room 和 Bootstrap API 都放在 HTTPS 后面，并把真实网页 Origin 精确
加入 `--allow-origin`。浏览器在非 `localhost` 的 HTTP 页面通常不会授予麦克风权限，HTTPS
页面也不能请求 HTTP API（Mixed Content）。RTC Token 应由业务 Token Service 按登录用户
短期签发；仓库内 `.env` 方案只用于开发和受控部署。

## HTTP 接口

```text
GET /healthz                 # 公共健康检查，不返回凭据
GET /api/v1/client/session   # 浏览器 RTC 启动配置；公网部署需要 Bearer Token
OPTIONS ...                  # 精确 Origin 的 CORS preflight
```

这是 Client Bootstrap API，不是 Studio API。服务端没有 Graph 写入、Node 源码、Run/Stop、
Observe 或 Connections 修改端点。

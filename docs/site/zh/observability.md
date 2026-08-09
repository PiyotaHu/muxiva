# 可观测性与堵点定位

实时语音链路出现“几秒后才出字幕”时，先不要猜是网络还是模型。Muxiva Studio 的 **Observe** 页面把 Runtime 的 Node 执行、Edge 背压以及 Node 内部缓冲放在同一张运行视图中。

## 第一次定位

1. 用 `./examples/voice-agent/run.sh` 启动项目，在 Studio 点击 **Run**。
2. 说一句完整的话，同时打开顶部的 **◎ Observe**。
3. 先看 **Hotspots**，再点击红色或黄色的 Node / Edge 行。
4. 右侧会展示导致判定的原始数值和建议动作。

| 看到的现象 | 它回答的问题 |
| --- | --- |
| Node rate、Processed | Node 有没有收到数据、当前每秒处理多少次 |
| Avg / Max process | `on_process` 本身是否耗时 |
| Edge rate、Frames | 数据是否真的经过这条连接 |
| Queue、Oldest | 下游是否来不及消费、最老数据已等待多久 |
| Drops / Full | 是否已经丢数据或阻塞生产者 |
| Media speed | Audio Edge 每秒传输了多少秒音频；`1.00×` 约等于实时速度 |
| Node metrics | Node 自己的隐藏缓冲，例如 RTC Source 内部缓存时长 |

状态阈值是面向实时交互的诊断默认值，而不是业务 SLA：Edge 队列达到 40% 或最老 Frame 等待 200 ms 会变黄；达到 80%、等待 1 s 或发生丢帧会变红。Node 平均处理超过 10 ms 会提示，超过 50 ms 会标红。Node 内部输入缓存达到 200 ms / 1 s 时也分别变黄 / 变红。

!!! important
    增大 Edge capacity 通常只能推迟失败，并会增加端到端延迟。先比较生产和消费速率，再处理慢 Node、阻塞 I/O 或 Node 内部没有及时排空的队列。

## 终端日志

Voice Demo 会把完整输出保存到项目本地的 `.muxiva/runtime.log`。运行期间可在另一个终端查看：

```bash
tail -f examples/voice-agent/.muxiva/runtime.log
```

每五秒会输出一次可机器检索的摘要；有堵点时同时输出具体实体：

```text
[MUXIVA][OBSERVE][SUMMARY] session=1 nodes=7 edges=8 queued=46 drops=0 bottlenecks=2 dashboard=Studio/Observe
[MUXIVA][OBSERVE][EDGE][CRITICAL] edge=audio-to-qwen queue=29/32 oldest_ms=1640 drops=0
[MUXIVA][OBSERVE][NODE][CRITICAL] node=agora-audio-source avg_process_ms=0.12 ingress_queue_ms=5120
```

快速过滤：

```bash
rg '\[MUXIVA\]\[OBSERVE\]|\[MUXIVA\].*(ERROR|WARN)' examples/voice-agent/.muxiva/runtime.log
```

## 跨会话历史趋势

Studio 每 5 秒保存一个有界快照，并在会话结束时保存最终快照。打开 **◎ Observe → Session history** 可以：

- 比较不同 Runtime Session 的总 Frame、最大积压、丢帧和最慢 Node 平均耗时；
- 点击一次历史会话，查看 Queued frames、Slowest Node avg、Drops 和 Frames processed 趋势；
- Studio 重启后继续查看历史。

采样由 Studio Server 后台执行，不依赖 Observe 页面保持打开。

数据保存在项目本地：

```text
.muxiva/observability/history.jsonl
```

同一份数据也通过 Bearer 鉴权 API 提供：`GET /api/v1/observability/history` 返回会话摘要，`GET /api/v1/observability/history/{run_id}` 返回该会话的采样点。

该目录默认被 Git 忽略。历史最多在内存中保留 5,000 个采样点；文件超过 16 MiB 时自动压缩到最近 2,500 个采样点。这份指标历史不会保存 Frame Payload、用户语音、对话文本或凭据。只有显式打开下面介绍的媒体 Dump 开关，才会保存原始音视频。

## 按轮次追踪每一条 Text、Event 和 Signal

打开 **◎ Observe → Semantic trace**，查看 Graph 中实际传播的语义，而不仅是性能计数器。最新 Turn 默认展开，每一行会显示：

- 相对 Runtime 启动时间；
- Text、Event 或 Signal 类型；
- 生产或消费它的 Node 与 Port；
- `OUT →` 或 `→ IN` 边界方向；
- 文本、Topic/Name 与 Payload 摘要。

点击一行，可以查看有界的完整 Payload，以及 Frame ID、Trace ID、Stream ID 和 Sequence。类型选择器与搜索框可按 Node、Port、Topic、文本片段、Frame 或 Trace 过滤。同一个 Frame ID 先出现在上游输出、再出现在下游输入，表示传播成功；缺少对应输入行，就能直接确定消息消失在哪个边界。

Studio 根据输出语义标记推导展示用 Turn：`muxiva.turn.started`、`muxiva.voice.speech.started`，以及其他 `*.turn.started` / `*.speech.started` 名称。标记本身属于新 Turn。同一标记同时以 Signal 和 Event 表达时，分组会去重，但两条 Trace 记录仍会分别保留。没有 Turn 标记的普通 Graph 会显示为一个 **Session flow**。这是 Studio 的展示逻辑，Runtime Core 不拥有、也不会切换业务 Turn ID。

语义追踪覆盖 Graph 的 Text/Event Frame Port，以及图内 Signal 控制平面，包括 Signal 的发出与送达。它不会把进程内 `NotificationBus` 消息冒充成 Graph Event；NotificationBus 仍通过其显式消费者或日志集成观察。

Trace 有明确上限：内存中保留最近 4 个会话，每个会话最多 10,000 条记录；单条展示 Payload 最多 4 KiB，单会话 Payload 最多 4 MiB。溢出与截断会在页面提示。对话内容在 Studio 重启时清空，不写入可观测性历史文件。鉴权 API 为 `GET /api/v1/observability/traces` 和 `GET /api/v1/observability/traces/<run-id>`。

!!! warning
    Semantic Trace 包含对话文本以及 Event/Signal 的结构化 Payload；截图和复制出的 JSON 都应按敏感数据处理。

## 检查每个 Node Port 的音频或视频

指标能回答“链路堵在哪里”，媒体 Dump 则能让你直接听到或看到“这个边界实际经过了什么”。打开 **◎ Observe → Node media dumps**，在运行前或运行中开启 **Dump Audio + Video**。Studio 每次启动时，该开关都默认关闭。

开启后，Studio 会按以下组合分别生成文件：

```text
Node + 输入/输出方向 + Port + 媒体格式
```

例如，`qwen-input-resampler.audio_out · OUTPUT` 是重采样 Node 实际发出的音频；`qwen-audio-realtime.audio_in · INPUT` 是 Qwen Node 实际收到的音频。对比相邻的两条轨道，就能直接发现数据损坏、静音、采样率错误以及 Node 之间的断点。

- Audio 会被封装成标准 WAV，可在 Observe 中直接播放或下载。
- RGBA8、YUV420p Video 会保存为带平面与 stride 元数据的原始帧序列，可在 Observe 的 Canvas 中回放或下载。
- 文件和 manifest 保存在 `.muxiva/observability/media/<run-id>/`，Studio 重启后仍可查看；最多保留最近 4 个会话。
- 采集使用容量为 256 Frame 的异步有界队列；单文件上限 64 MiB，单会话上限 256 MiB。诊断队列满时只丢弃 Dump 副本，不会丢 Runtime Frame、不会阻塞实时链路；页面会显示丢弃和截断数量。

!!! warning
    Dump 可能包含隐私敏感的麦克风音频或摄像头画面。只在排障期间开启，用完立即关闭，也不要发布 `.muxiva/observability/media`。项目模板默认 Git Ignore 该目录，但复制或下载后的文件仍需由你负责保护。

对应的鉴权 API 为：`GET /api/v1/observability/media`、`GET /api/v1/observability/media/<run-id>`、`PUT /api/v1/observability/media`（请求体 `{"enabled":true|false}`），以及 `GET /api/v1/observability/media-artifacts/<run-id>/<artifact-id>`。

## Prometheus 抓取

Studio 提供标准 Prometheus 文本端点：

```text
GET /metrics
```

它与 Studio API 使用同一个 Bearer Token。手工验证：

```bash
# Token 是 Studio 启动 URL 中 # 后面的值
curl -H "Authorization: Bearer $MUXIVA_STUDIO_ACCESS_TOKEN" \
  http://127.0.0.1:5678/metrics
```

生产式本地抓取应固定端口和私有 Token：

```bash
export MUXIVA_STUDIO_ACCESS_TOKEN="$(openssl rand -hex 32)"
./examples/voice-agent/run.sh --port 5678
```

Prometheus 配置示例：

```yaml
scrape_configs:
  - job_name: muxiva-local
    static_configs:
      - targets: ["127.0.0.1:5678"]
    authorization:
      type: Bearer
      credentials: "替换为 MUXIVA_STUDIO_ACCESS_TOKEN"
```

主要指标使用 `muxiva_node_*` 和 `muxiva_edge_*` 前缀，包括 Node 回调、耗时、错误、自定义 Counter/Gauge，以及 Edge 队列、吞吐、丢帧和阻塞。

## OpenTelemetry OTLP/HTTP

OTLP 导出器位于 Studio 应用层，不会把 OpenTelemetry SDK 或网络客户端耦合进 Runtime Core。当前支持标准的 **OTLP/HTTP JSON**，向指标端点发送 `ExportMetricsServiceRequest` JSON；默认基础端点会追加 `/v1/metrics`。协议规则见 [OpenTelemetry OTLP 规范](https://opentelemetry.io/docs/specs/otlp/)。

连接本地 OpenTelemetry Collector：

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:4318"
export OTEL_EXPORTER_OTLP_PROTOCOL="http/json"
export OTEL_METRIC_EXPORT_INTERVAL="10000"
./examples/voice-agent/run.sh
```

厂商提供完整 Metrics URL 时使用 signal-specific 配置；它不会自动追加路径：

```bash
export OTEL_EXPORTER_OTLP_METRICS_ENDPOINT="https://collector.example.com/v1/metrics"
export OTEL_EXPORTER_OTLP_METRICS_PROTOCOL="http/json"
export OTEL_EXPORTER_OTLP_METRICS_HEADERS="authorization=Bearer%20YOUR_TOKEN"
```

Observe 的 Session history 标题栏会显示 `OTLP configured`、`exporting` 或最近一次错误。导出在独立线程执行，不阻塞 Node 回调和 Runtime 调度；上一次请求未结束时不会无限堆积新请求。

!!! note
    当前实现只导出 Metrics，不导出 Trace 和 Log；采样历史是项目本地开发能力，不代替 Prometheus、Tempo、Jaeger 或厂商后台的长期保留。

## 自定义 Node 暴露内部指标

Runtime 自动测量回调和 Edge，但它无法猜到 SDK 或模型客户端内部还有一层队列。Node 应通过 `ctx` 上报非敏感的整数 Counter / Gauge；这些值会出现在 Observe 的 **Node metrics** 中。

=== "Python"

    ```python
    ctx.increment_counter("ingress.received_frames")
    ctx.set_gauge("ingress.queue_duration_ms", buffered_ms)
    ```

=== "TypeScript"

    ```typescript
    ctx.incrementCounter("ingress.received_frames")
    ctx.setGauge("ingress.queue_duration_ms", bufferedMs)
    ```

=== "Rust"

    ```rust
    context.increment_counter("ingress.received_frames", 1)?;
    context.set_gauge("ingress.queue_duration_ms", buffered_ms)?;
    ```

=== "C++"

    ```cpp
    ctx.increment_counter("ingress.received_frames");
    ctx.set_gauge("ingress.queue_duration_ms", buffered_ms);
    ```

名称只能包含 ASCII 字母、数字、点、下划线和连字符，最长 64 字符；值必须是非负整数。不要把 API Key、原始语音或用户文本写入指标。

## 如何区分三类延迟

- Node 平均处理时间高、输入 Edge 堆积：算子回调慢或在同步等待网络/磁盘。
- Node 处理很快，但它报告的 `ingress.queue_duration_ms` 高：算子内部 SDK 队列或轮询节奏有 bug。
- Agora/Qwen 输入 Frame 持续增长，但 `input.audio_peak_pcm16` 和
  `input.audio_mean_abs_pcm16` 始终接近 0：浏览器发布的是静音或错误输入设备；先修复麦克风，
  不要排查模型网络。
- Runtime 所有队列健康，而云端首包仍慢：重点检查厂商服务时延、地域、会话配置与网络；结合厂商侧 request/session ID 排查。

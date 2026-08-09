# Observability and bottleneck diagnosis

When captions arrive seconds late, do not begin by guessing whether the network or model is slow. Muxiva Studio's **Observe** page correlates Runtime Node execution, Edge backpressure, and Node-owned buffers in one live view.

## First diagnosis

1. Open the local diagnostic environment with `muxiva studio examples/voice-agent/graph.json`, then select **Run** in Studio.
2. Speak one complete utterance and open **◎ Observe** in the top bar.
3. Start with **Hotspots**, then click a red or yellow Node / Edge row.
4. The details pane shows the measurements behind the verdict and a concrete next action.

| Measurement | Question it answers |
| --- | --- |
| Node rate, Processed | Is the Node receiving work, and how many callbacks run per second? |
| Avg / Max process | Is `on_process` itself slow? |
| Edge rate, Frames | Is data actually crossing this connection? |
| Queue, Oldest | Is the consumer falling behind, and how long has the oldest Frame waited? |
| Drops / Full | Has data already been lost or has the producer blocked? |
| Media speed | How many seconds of Audio cross the Edge each wall-clock second; `1.00×` is approximately real time. |
| Node metrics | Hidden Node-owned buffers, such as an RTC Source's internal queue duration. |

The health thresholds are real-time interaction defaults, not an application SLA. An Edge turns yellow at 40% queue occupancy or 200 ms oldest-frame age; it turns red at 80%, 1 s, or any drop. A Node warns above 10 ms average process time and turns red above 50 ms. Node-owned ingress buffers warn at 200 ms and turn red at 1 s.

!!! important
    Increasing Edge capacity usually postpones failure while adding end-to-end latency. Compare production and consumption rates first, then fix the slow Node, blocking I/O, or a Node-owned queue that is not being drained.

## Terminal logs

The Voice Demo writes its full output to the project's local `.muxiva/runtime.log`. Follow it from another terminal:

```bash
tail -f examples/voice-agent/.muxiva/runtime.log
```

Muxiva emits a searchable summary every five seconds and names unhealthy entities:

```text
[MUXIVA][OBSERVE][SUMMARY] session=1 nodes=7 edges=8 queued=46 drops=0 bottlenecks=2 dashboard=Studio/Observe
[MUXIVA][OBSERVE][EDGE][CRITICAL] edge=audio-to-qwen queue=29/32 oldest_ms=1640 drops=0
[MUXIVA][OBSERVE][NODE][CRITICAL] node=agora-audio-source avg_process_ms=0.12 ingress_queue_ms=5120
```

Filter the log quickly:

```bash
rg '\[MUXIVA\]\[OBSERVE\]|\[MUXIVA\].*(ERROR|WARN)' examples/voice-agent/.muxiva/runtime.log
```

## Top-level Session selector and cross-session trends

Observe starts with a Session dropdown. Each option is keyed by `Session #<session_id>` and adds
the start time and RTC channel as context. Once selected, Summary, Hotspots, Semantic Trace,
Media Dump, Nodes, Edges, and the detail pane all read that Session only; the current Runtime is
never mixed into a historical view.

Studio persists a bounded snapshot every five seconds and a final snapshot when a session terminates. Use the top selector to:

- compare total Frames, peak backlog, drops, and slowest Node average across Runtime sessions;
- select a historical session and inspect Queued frames, Slowest Node avg, Drops, and Frames processed trends;
- retain history across Studio restarts.

The Studio Server samples in the background; the Observe page does not need to remain open.

Data is local to the project:

```text
.muxiva/observability/history.jsonl
```

The same data is available through Bearer-authenticated APIs: `GET /api/v1/observability/history` returns session summaries and `GET /api/v1/observability/history/{run_id}` returns one session's samples.

The directory is Git ignored by default. Memory retains at most 5,000 samples; when the file exceeds 16 MiB it is compacted to the newest 2,500 samples. This metrics history never stores Frame payloads, user audio, conversation text, or credentials. Raw media is stored only when you explicitly enable the separate media-dump switch described below.

## Follow every Text, Event, and Signal by turn

Open **◎ Observe → Semantic trace** to inspect the meaning flowing through the Graph rather than only its performance counters. The newest turn is expanded first. Each row shows:

- elapsed time from Runtime start;
- Text, Event, or Signal type;
- producing or consuming Node and Port;
- `OUT →` or `→ IN` boundary direction;
- text/topic/name and a payload summary.

Click a row to inspect the full bounded payload plus Frame ID, Trace ID, Stream ID, and Sequence. Use the type selector or search box to isolate one Node, Port, topic, text fragment, Frame, or Trace. Seeing the same Frame ID as an output and then as the next Node's input confirms propagation; a missing input row identifies the exact boundary where it stopped.

Studio derives display turns from output markers named `muxiva.turn.started`, `muxiva.voice.speech.started`, or another `*.turn.started` / `*.speech.started` name. The marker itself is included in the turn. Duplicate Signal/Event representations of the same marker are deduplicated for grouping, but both remain visible as separate trace rows. A Graph without turn markers is shown as one **Session flow** group. This is presentation logic in Studio—Runtime Core does not own or mutate a business Turn ID.

Semantic tracing covers Graph Text/Event Frame ports and the graph-local Signal control plane, including Signal emission and delivery. It deliberately does **not** label process-local `NotificationBus` messages as Graph Events; those remain visible through their explicit NotificationBus consumer/logging integration.

Trace storage is bounded to four in-memory sessions, 10,000 entries per session, 4 KiB per displayed payload, and 4 MiB of payload data per session. Overflow and truncation are shown in the UI. Conversation contents are cleared when Studio restarts and are not written into observability history. The authenticated APIs are `GET /api/v1/observability/traces` and `GET /api/v1/observability/traces/<run-id>`.

!!! warning
    Semantic traces contain conversation text and structured Event/Signal payloads. Treat screenshots and copied JSON as sensitive data.

## Inspect the Audio or Video at every Node Port

Metrics tell you **where** a flow stopped; a media dump lets you hear or see **what** crossed that boundary. Open **◎ Observe → Node media dumps** and enable **Dump Audio + Video** before or during a run. The switch is off whenever Studio starts.

When enabled, Studio creates a separate artifact for every observed combination of:

```text
Node + input/output direction + Port + media format
```

For example, `qwen-input-resampler.audio_out · OUTPUT` is the exact Audio emitted by the resampler, while `qwen-audio-realtime.audio_in · INPUT` is what the Qwen Node actually received. Comparing the two tracks makes corruption, silence, wrong sample rates, and breaks between adjacent Nodes immediately visible.

- Audio is finalized as a standard WAV file and can be played or downloaded directly in Observe.
- RGBA8 and YUV420p Video is stored as a raw frame sequence with plane/stride metadata and can be replayed on the Observe canvas or downloaded.
- Artifacts and a manifest survive a Studio restart under `.muxiva/observability/media/<run-id>/`; only the newest four sessions are retained.
- Capture uses a bounded 256-Frame asynchronous queue, a 64 MiB limit per artifact, and a 256 MiB limit per session. A full diagnostic queue drops only dump copies—not Runtime Frames—and the UI reports the drop/truncation count.

!!! warning
    A dump may contain private microphone audio or camera frames. Enable it only while diagnosing, stop it afterwards, and do not publish `.muxiva/observability/media`. The directory is Git ignored by the project templates, but you remain responsible for copied or downloaded files.

The authenticated APIs are `GET /api/v1/observability/media`, `GET /api/v1/observability/media/<run-id>`, `PUT /api/v1/observability/media` with `{"enabled":true|false}`, and `GET /api/v1/observability/media-artifacts/<run-id>/<artifact-id>`.

## Prometheus scraping

Studio exposes the standard Prometheus text endpoint:

```text
GET /metrics
```

It uses the same Bearer Token as the Studio API. Verify it manually:

```bash
# The token follows # in the Studio startup URL.
curl -H "Authorization: Bearer $MUXIVA_STUDIO_ACCESS_TOKEN" \
  http://127.0.0.1:5678/metrics
```

Use a fixed port and private token for a stable local scrape target:

```bash
export MUXIVA_STUDIO_ACCESS_TOKEN="$(openssl rand -hex 32)"
./examples/voice-agent/run.sh --port 5678
```

Example Prometheus configuration:

```yaml
scrape_configs:
  - job_name: muxiva-local
    static_configs:
      - targets: ["127.0.0.1:5678"]
    authorization:
      type: Bearer
      credentials: "replace with MUXIVA_STUDIO_ACCESS_TOKEN"
```

The principal metric families use `muxiva_node_*` and `muxiva_edge_*`: Node callbacks, latency, errors and custom counters/gauges; Edge queue state, throughput, drops, and blocking.

## OpenTelemetry OTLP/HTTP

The exporter lives in the Studio application layer, keeping the OpenTelemetry SDK and network client out of Runtime Core. It currently supports standard **OTLP/HTTP JSON** and sends an `ExportMetricsServiceRequest` JSON document to the metrics endpoint. A base endpoint receives the standard `/v1/metrics` suffix. See the [OpenTelemetry OTLP specification](https://opentelemetry.io/docs/specs/otlp/).

Connect a local OpenTelemetry Collector:

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT="http://127.0.0.1:4318"
export OTEL_EXPORTER_OTLP_PROTOCOL="http/json"
export OTEL_METRIC_EXPORT_INTERVAL="10000"
./examples/voice-agent/run.sh
```

Use the signal-specific variable for a complete vendor Metrics URL; no path is appended:

```bash
export OTEL_EXPORTER_OTLP_METRICS_ENDPOINT="https://collector.example.com/v1/metrics"
export OTEL_EXPORTER_OTLP_METRICS_PROTOCOL="http/json"
export OTEL_EXPORTER_OTLP_METRICS_HEADERS="authorization=Bearer%20YOUR_TOKEN"
```

The area below the top Session selector reports `OTLP configured`, `exporting`, or the most recent error. Export happens on a separate thread and never blocks Node callbacks or Runtime scheduling; an unfinished export cannot create an unbounded request backlog.

!!! note
    This implementation exports Metrics only, not Traces or Logs. Project-local history is a development feature and does not replace long-term retention in Prometheus, Tempo, Jaeger, or a hosted backend.

## Expose internals from a custom Node

The Runtime measures callbacks and Edges automatically, but cannot infer a queue hidden inside an SDK or model client. A Node should report non-sensitive integer counters and gauges through `ctx`; they appear under **Node metrics** in Observe.

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

Names are limited to 64 ASCII letters, digits, dots, underscores, and hyphens. Values are non-negative integers. Never put API keys, raw audio, or user text in metrics.

## Distinguish three kinds of latency

- High Node process time plus a growing input Edge: the callback is slow or synchronously waiting on network/disk I/O.
- Fast Node callbacks but high `ingress.queue_duration_ms`: the SDK queue or Node polling cadence is faulty.
- Agora/Qwen input Frames advance but `input.audio_peak_pcm16` and
  `input.audio_mean_abs_pcm16` stay near zero: the browser is publishing silence or the wrong
  input device; fix microphone input before investigating model networking.
- Healthy Runtime queues but a slow cloud first byte: inspect provider latency, region, session configuration, and the network, correlated with the provider request/session ID.

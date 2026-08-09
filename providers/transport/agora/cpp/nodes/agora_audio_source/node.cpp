#include <muxiva/agora_rtc.hpp>
#include <muxiva/muxiva.hpp>

#include <atomic>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <string>

namespace {
std::string required_env(const char* name) {
  const char* value = std::getenv(name);
  if (value == nullptr || *value == '\0') throw std::runtime_error(std::string("missing ") + name);
  return value;
}

muxiva_str_v1 borrow(const std::string& value) noexcept {
  return {value.data(), value.size()};
}

class AgoraAudioSourceNode final : public muxiva::MultimodalGraphNode {
 public:
  void on_prepare() override {
    const auto app_id = required_env("MUXIVA_AGORA_APP_ID");
    const auto token = required_env("MUXIVA_AGORA_BOT_TOKEN");
    const auto channel = required_env("MUXIVA_AGORA_CHANNEL");
    const auto uid = static_cast<std::uint32_t>(std::stoul(required_env("MUXIVA_AGORA_BOT_UID")));
    const auto participant = static_cast<std::uint32_t>(
        std::stoul(required_env("MUXIVA_AGORA_WEB_UID")));
    session_ = muxiva::agora::SharedSession::acquire(
        app_id, token, channel, uid, participant);
  }

  void on_process(const muxiva_frame_view_v1*, muxiva::GraphNodeContext& ctx) override {
    // Agora delivers 10 ms PCM packets. Poll at that cadence and drain a bounded
    // burst so scheduler jitter can never turn the SDK callback queue into a
    // multi-second hidden latency buffer.
    ctx.schedule_next_tick(std::chrono::milliseconds(10));
    if (session_) {
      const auto stats = session_->audio_ingress_stats();
      if (stats.received_total >= last_received_total_) {
        ctx.increment_counter("ingress.received_frames",
                              stats.received_total - last_received_total_);
      }
      if (stats.dropped_total >= last_dropped_total_) {
        ctx.increment_counter("ingress.dropped_frames",
                              stats.dropped_total - last_dropped_total_);
      }
      last_received_total_ = stats.received_total;
      last_dropped_total_ = stats.dropped_total;
      ctx.set_gauge("ingress.queue_frames", stats.queued_frames);
      ctx.set_gauge("ingress.queue_duration_ms",
                    stats.queued_duration_ns / 1000000ULL);
    }
    if (!session_) return;
    for (std::size_t drained = 0;
         drained < kMaxDrainPerTick && session_->try_pop_audio(current_);
         ++drained) {
      emit_current(ctx);
    }
  }

  void on_finish() override {
    session_.reset();
  }

  void on_abort(const muxiva_abort_reason_v1&) noexcept override {
    try { on_finish(); } catch (...) {}
  }

 private:
  void emit_current(muxiva::GraphNodeContext& ctx) {
    frame_ = {};
    frame_.header.abi_version = MUXIVA_ABI_VERSION_V1;
    frame_.header.struct_size = sizeof(frame_.header);
    frame_.header.frame_type = MUXIVA_FRAME_AUDIO;
    frame_.header.clock_kind = MUXIVA_CLOCK_MONOTONIC;
    frame_.header.timestamp_ns = current_.timestamp_ms * 1000000;
    frame_.header.sequence_id = ++sequence_;
    frame_id_ = "agora-audio-" + std::to_string(sequence_);
    stream_id_ = "agora-remote-" + std::to_string(current_.remote_uid);
    trace_id_ = frame_id_;
    frame_.header.frame_id = borrow(frame_id_);
    frame_.header.clock_domain_id = borrow(clock_domain_);
    frame_.header.stream_id = borrow(stream_id_);
    frame_.header.trace_id = borrow(trace_id_);
    frame_.payload.audio.sample_rate_hz = current_.sample_rate_hz;
    frame_.payload.audio.channels = current_.channels;
    frame_.payload.audio.sample_format = MUXIVA_PCM_I16LE;
    frame_.payload.audio.layout = MUXIVA_AUDIO_INTERLEAVED;
    frame_.payload.audio.samples_per_channel = current_.samples_per_channel;
    frame_.payload.audio.bytes = {current_.bytes.data(), current_.bytes.size()};
    ctx.emit("audio_out", frame_);
    ctx.increment_counter("output.audio_frames", 1);
    const auto emitted = ++emitted_frames_;
    if (emitted == 1 || emitted % 500 == 0) {
      std::fprintf(stderr,
                   "[MUXIVA][AGORA][audio.forwarded] frames=%llu bytes=%zu "
                   "participant_uid=%u\n",
                   static_cast<unsigned long long>(emitted), current_.bytes.size(),
                   current_.remote_uid);
    }
  }
  static constexpr std::size_t kMaxDrainPerTick = 8;
  std::shared_ptr<muxiva::agora::SharedSession> session_;
  muxiva::agora::OwnedPcm16Frame current_;
  muxiva_frame_view_v1 frame_{};
  std::uint64_t sequence_ = 0;
  std::atomic<std::uint64_t> emitted_frames_{0};
  std::uint64_t last_received_total_ = 0;
  std::uint64_t last_dropped_total_ = 0;
  std::string frame_id_;
  std::string clock_domain_ = "agora.remote.monotonic";
  std::string stream_id_;
  std::string trace_id_;
};
}  // namespace

extern "C" muxiva_multimodal_node_factory_v1 muxiva_node_pack_factory() {
  static const auto factory = muxiva::MultimodalGraphNodeFactory::make<AgoraAudioSourceNode>(
      "agora.audio_source", MUXIVA_NODE_SOURCE,
      R"json([{"name":"audio_out","direction":"output","frameType":"audio"}])json",
      "{}", "1.1.0");
  return factory.view();
}

#include "muxiva/rtc_adapter_v1.h"

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <deque>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace {
using Clock = std::chrono::steady_clock;

void write_error(muxiva_error_v1* out, muxiva_status_v1 status, const char* code,
                 const char* message) noexcept {
  if (!out) return;
  out->status = status;
  out->category = status == MUXIVA_STATUS_INVALID_ARGUMENT ? MUXIVA_ERROR_CATEGORY_VALIDATION
                                                          : MUXIVA_ERROR_CATEGORY_LIFECYCLE;
  std::strncpy(out->code, code, sizeof(out->code) - 1);
  std::strncpy(out->message, message, sizeof(out->message) - 1);
}

bool valid_prefix(uint32_t abi, uint32_t size, size_t expected) noexcept {
  return abi == MUXIVA_ABI_VERSION_V1 && size >= expected;
}
bool valid_view(const void* data, size_t len, size_t max) noexcept {
  return len <= max && (len == 0 || data != nullptr);
}

struct Event {
  enum Kind { Audio, Video, Text, State, Participant, Error, Interrupt, Notification } kind;
  uint64_t sequence = 0;
  int64_t timestamp_ns = 0;
  uint32_t a = 0, b = 0, c = 0, d = 0;
  uint64_t wide = 0;
  std::vector<uint8_t> bytes;
  std::string text;
};

struct Context {
  muxiva_rtc_callbacks_v1 callbacks{};
  void* user_data = nullptr;
  muxiva_session_ingress_v1 ingress{};
  std::atomic<bool> accepting{true};
  std::atomic<uint64_t> in_flight{0}, accepted{0}, full{0}, closed{0}, invalid{0}, late{0};
  std::atomic<uint64_t> last_sequence{0}, callback_thread_hash{0};
  std::mutex drain_mutex;
  std::condition_variable drain_cv;
  std::mutex barrier_mutex;
  std::condition_variable barrier_cv;
  bool barrier_held = false;

  ~Context() { (void)muxiva_session_ingress_release_v1(ingress); }
};

struct Flight {
  std::shared_ptr<Context> context;
  explicit Flight(std::shared_ptr<Context> value) : context(std::move(value)) {
    context->in_flight.fetch_add(1, std::memory_order_acq_rel);
  }
  ~Flight() {
    context->in_flight.fetch_sub(1, std::memory_order_acq_rel);
    context->drain_cv.notify_all();
  }
};

void submit_media(const std::shared_ptr<Context>& ctx, const Event& event) noexcept {
  Flight flight(ctx);
  ctx->callback_thread_hash.store(std::hash<std::thread::id>{}(std::this_thread::get_id()), std::memory_order_release);
  {
    std::unique_lock<std::mutex> lock(ctx->barrier_mutex);
    ctx->barrier_cv.wait(lock, [&] { return !ctx->barrier_held; });
  }
  if (!ctx->accepting.load(std::memory_order_acquire)) {
    ctx->late.fetch_add(1, std::memory_order_relaxed);
    return;
  }
  muxiva_frame_view_v1 frame{};
  frame.header.abi_version = MUXIVA_ABI_VERSION_V1;
  frame.header.struct_size = sizeof(frame.header);
  frame.header.clock_kind = MUXIVA_CLOCK_MEDIA_RELATIVE;
  frame.header.timestamp_ns = event.timestamp_ns;
  frame.header.sequence_id = event.sequence;
  static const char stream[] = "rtc.remote";
  static const char clock[] = "rtc.media";
  static const char frame_id[] = "rtc.frame";
  static const char trace[] = "rtc.trace";
  frame.header.frame_id = {frame_id, sizeof(frame_id) - 1};
  frame.header.stream_id = {stream, sizeof(stream) - 1};
  frame.header.clock_domain_id = {clock, sizeof(clock) - 1};
  frame.header.trace_id = {trace, sizeof(trace) - 1};
  if (event.kind == Event::Audio) {
    frame.header.frame_type = MUXIVA_FRAME_AUDIO;
    frame.payload.audio = {event.a, static_cast<uint16_t>(event.b), static_cast<uint16_t>(event.c), event.d, 0, event.wide, {event.bytes.data(), event.bytes.size()}, {0, 0}};
  } else if (event.kind == Event::Video) {
    frame.header.frame_type = MUXIVA_FRAME_VIDEO;
    frame.payload.video = {event.a, event.b, event.c, event.d, {event.bytes.data(), event.bytes.size()}, {0, 0, 0, 0}};
  } else {
    frame.header.frame_type = MUXIVA_FRAME_TEXT;
    static const char media_type[] = "text/plain";
    frame.payload.text = {{event.text.data(), event.text.size()}, {media_type, sizeof(media_type) - 1}, {0, 0}};
  }
  muxiva_error_v1 error{};
  error.abi_version = MUXIVA_ABI_VERSION_V1;
  error.struct_size = sizeof(error);
  const auto status = muxiva_session_ingress_try_submit_v1(ctx->ingress, &frame, &error);
  if (status == MUXIVA_STATUS_OK) ctx->accepted.fetch_add(1, std::memory_order_relaxed);
  else if (status == MUXIVA_STATUS_QUEUE_FULL) ctx->full.fetch_add(1, std::memory_order_relaxed);
  else if (status == MUXIVA_STATUS_CLOSED) ctx->closed.fetch_add(1, std::memory_order_relaxed);
  else ctx->invalid.fetch_add(1, std::memory_order_relaxed);
  if (ctx->callbacks.on_media) (void)ctx->callbacks.on_media(ctx->user_data, &frame, &error);
  ctx->last_sequence.store(event.sequence, std::memory_order_release);
}

void deliver_control(const std::shared_ptr<Context>& ctx, const Event& event) noexcept {
  Flight flight(ctx);
  if (!ctx->accepting.load(std::memory_order_acquire)) { ctx->late.fetch_add(1, std::memory_order_relaxed); return; }
  muxiva_error_v1 error{};
  error.abi_version = MUXIVA_ABI_VERSION_V1;
  error.struct_size = sizeof(error);
  switch (event.kind) {
    case Event::State: { muxiva_rtc_state_v1 value{MUXIVA_ABI_VERSION_V1, sizeof(value), event.a, 0, event.sequence}; if (ctx->callbacks.on_connection_state) (void)ctx->callbacks.on_connection_state(ctx->user_data, &value, &error); break; }
    case Event::Participant: { muxiva_str_v1 name{event.text.data(), event.text.size()}; muxiva_rtc_participant_event_v1 value{MUXIVA_ABI_VERSION_V1, sizeof(value), event.a, 0, name, event.sequence}; if (ctx->callbacks.on_participant) (void)ctx->callbacks.on_participant(ctx->user_data, &value, &error); break; }
    case Event::Error: { muxiva_str_v1 message{event.text.data(), event.text.size()}; muxiva_rtc_error_event_v1 value{MUXIVA_ABI_VERSION_V1, sizeof(value), static_cast<int32_t>(event.a), 0, message, event.sequence}; if (ctx->callbacks.on_error) (void)ctx->callbacks.on_error(ctx->user_data, &value, &error); break; }
    case Event::Interrupt: { muxiva_str_v1 name{event.text.data(), event.text.size()}; muxiva_rtc_interrupt_event_v1 value{MUXIVA_ABI_VERSION_V1, sizeof(value), name, {event.bytes.data(), event.bytes.size()}, event.sequence}; if (ctx->callbacks.on_interrupt) (void)ctx->callbacks.on_interrupt(ctx->user_data, &value, &error); break; }
    case Event::Notification: { muxiva_str_v1 topic{event.text.data(), event.text.size()}; muxiva_rtc_notification_v1 value{MUXIVA_ABI_VERSION_V1, sizeof(value), topic, {event.bytes.data(), event.bytes.size()}, event.sequence}; if (ctx->callbacks.on_notification) (void)ctx->callbacks.on_notification(ctx->user_data, &value, &error); break; }
    default: break;
  }
  muxiva_frame_view_v1 frame{};
  frame.header.abi_version = MUXIVA_ABI_VERSION_V1;
  frame.header.struct_size = sizeof(frame.header);
  frame.header.clock_kind = MUXIVA_CLOCK_MONOTONIC;
  frame.header.sequence_id = event.sequence;
  static const char frame_id[] = "rtc.control";
  static const char clock[] = "rtc.control.clock";
  static const char stream[] = "rtc.control.stream";
  static const char trace[] = "rtc.control.trace";
  static const char source[] = "rtc.adapter";
  static const uint8_t schema[] = "{\"schema_version\":1}";
  frame.header.frame_id = {frame_id, sizeof(frame_id) - 1};
  frame.header.clock_domain_id = {clock, sizeof(clock) - 1};
  frame.header.stream_id = {stream, sizeof(stream) - 1};
  frame.header.trace_id = {trace, sizeof(trace) - 1};
  if (event.kind == Event::Error || event.kind == Event::Notification) {
    static const char error_topic[] = "rtc.error";
    static const char notification_topic[] = "rtc.notification";
    const char* topic = event.kind == Event::Error ? error_topic : notification_topic;
    const size_t topic_len = event.kind == Event::Error ? sizeof(error_topic) - 1 : sizeof(notification_topic) - 1;
    frame.header.frame_type = MUXIVA_FRAME_EVENT;
    frame.payload.event = {{topic, topic_len}, {schema, sizeof(schema) - 1}, {0, 0}};
  } else {
    static const char state_name[] = "rtc.connection_state";
    static const char participant_name[] = "rtc.participant";
    static const char interrupt_name[] = "rtc.interrupt";
    const char* name = state_name;
    size_t name_len = sizeof(state_name) - 1;
    if (event.kind == Event::Participant) { name = participant_name; name_len = sizeof(participant_name) - 1; }
    if (event.kind == Event::Interrupt) { name = interrupt_name; name_len = sizeof(interrupt_name) - 1; }
    frame.header.frame_type = MUXIVA_FRAME_SIGNAL;
    frame.payload.signal = {{name, name_len}, {source, sizeof(source) - 1}, {schema, sizeof(schema) - 1}, {0, 0}};
  }
  const auto status = muxiva_session_ingress_try_submit_v1(ctx->ingress, &frame, &error);
  if (status == MUXIVA_STATUS_OK) ctx->accepted.fetch_add(1, std::memory_order_relaxed);
  else if (status == MUXIVA_STATUS_QUEUE_FULL) ctx->full.fetch_add(1, std::memory_order_relaxed);
  else if (status == MUXIVA_STATUS_CLOSED) ctx->closed.fetch_add(1, std::memory_order_relaxed);
  else ctx->invalid.fetch_add(1, std::memory_order_relaxed);
  ctx->last_sequence.store(event.sequence, std::memory_order_release);
}
}

struct muxiva_rtc_adapter_handle_v1 {
  std::mutex mutex;
  std::condition_variable cv;
  std::deque<Event> events;
  std::shared_ptr<Context> context;
  std::thread worker;
  uint32_t state = MUXIVA_RTC_CREATED;
  size_t max_bytes = 0;
  uint32_t drain_ms = 0;
  muxiva_rtc_mock_faults_v1 faults{};
  uint64_t scheduled = 0;
  bool stop = false;
  bool preserve_late = false;

  void schedule(Event event) {
    std::lock_guard<std::mutex> lock(mutex);
    if (stop && !preserve_late) return;
    event.sequence = ++scheduled;
    if (faults.drop_every_n && event.sequence % faults.drop_every_n == 0) return;
    if (faults.reorder_window > 1 && event.sequence % faults.reorder_window == 0) events.push_front(std::move(event));
    else events.push_back(std::move(event));
    cv.notify_one();
  }
};

namespace {
void worker_loop(muxiva_rtc_adapter_handle_v1* adapter) noexcept {
  for (;;) {
    Event event;
    {
      std::unique_lock<std::mutex> lock(adapter->mutex);
      adapter->cv.wait(lock, [&] { return adapter->stop || !adapter->events.empty(); });
      if (adapter->events.empty() && adapter->stop) return;
      event = std::move(adapter->events.front());
      adapter->events.pop_front();
    }
    if (adapter->faults.delay_ms) std::this_thread::sleep_for(std::chrono::milliseconds(adapter->faults.delay_ms));
    if (event.kind == Event::Audio || event.kind == Event::Video || event.kind == Event::Text) submit_media(adapter->context, event);
    else deliver_control(adapter->context, event);
    if (adapter->faults.disconnect_after_n && event.sequence == adapter->faults.disconnect_after_n) {
      Event disconnected; disconnected.kind = Event::State; disconnected.a = MUXIVA_RTC_FAILED; adapter->schedule(std::move(disconnected));
    }
  }
}

template <typename T> bool packet_prefix(const T* packet) { return packet && valid_prefix(packet->abi_version, packet->struct_size, sizeof(T)); }

muxiva_status_v1 state_error(muxiva_error_v1* error, const char* message) noexcept {
  write_error(error, MUXIVA_STATUS_CLOSED, "MUXIVA-RTC-STATE", message); return MUXIVA_STATUS_CLOSED;
}
}

extern "C" muxiva_status_v1 muxiva_rtc_adapter_create_v1(const muxiva_rtc_adapter_config_v1* config, const muxiva_rtc_callbacks_v1* callbacks, void* user_data, muxiva_rtc_adapter_handle_v1** out, muxiva_error_v1* error) {
 try {
  if (!out || !config || !callbacks || !valid_prefix(config->abi_version, config->struct_size, sizeof(*config)) || !valid_prefix(callbacks->abi_version, callbacks->struct_size, sizeof(*callbacks)) || !callbacks->on_media || !callbacks->on_connection_state || !callbacks->on_participant || !callbacks->on_error || !callbacks->on_interrupt || !callbacks->on_notification || config->max_packet_bytes == 0 || config->max_packet_bytes > 16u * 1024u * 1024u || !valid_prefix(config->faults.abi_version, config->faults.struct_size, sizeof(config->faults))) { write_error(error, MUXIVA_STATUS_INVALID_ARGUMENT, "MUXIVA-RTC-CONFIG", "invalid adapter configuration"); return MUXIVA_STATUS_INVALID_ARGUMENT; }
  muxiva_session_ingress_v1 retained{};
  if (muxiva_session_ingress_clone_v1(config->ingress, &retained, error) != MUXIVA_STATUS_OK) return error ? error->status : MUXIVA_STATUS_INVALID_HANDLE;
  auto adapter = std::make_unique<muxiva_rtc_adapter_handle_v1>();
  adapter->max_bytes = config->max_packet_bytes; adapter->drain_ms = config->callback_drain_timeout_ms; adapter->faults = config->faults;
  adapter->context = std::make_shared<Context>(); adapter->context->callbacks = *callbacks; adapter->context->user_data = user_data; adapter->context->ingress = retained; adapter->context->barrier_held = config->faults.hold_callback_entry != 0;
  adapter->worker = std::thread(worker_loop, adapter.get());
  *out = adapter.release(); return MUXIVA_STATUS_OK;
 } catch (...) { write_error(error, MUXIVA_STATUS_FOREIGN_EXCEPTION, "MUXIVA-RTC-EXCEPTION", "C++ exception caught at create boundary"); return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
}

extern "C" muxiva_status_v1 muxiva_rtc_adapter_connect_v1(muxiva_rtc_adapter_handle_v1* adapter, const muxiva_rtc_join_request_v1* request, muxiva_error_v1* error) {
 try { if (!adapter || !packet_prefix(request) || !valid_view(request->channel.data, request->channel.len, 255) || request->channel.len == 0 || !valid_view(request->participant_id.data, request->participant_id.len, 255)) { write_error(error, MUXIVA_STATUS_INVALID_ARGUMENT, "MUXIVA-RTC-JOIN", "invalid join request"); return MUXIVA_STATUS_INVALID_ARGUMENT; }
  { std::lock_guard<std::mutex> lock(adapter->mutex); if (adapter->state != MUXIVA_RTC_CREATED) return state_error(error, "adapter is not in Created"); adapter->state = MUXIVA_RTC_CONNECTING; }
  Event connecting; connecting.kind = Event::State; connecting.a = MUXIVA_RTC_CONNECTING; adapter->schedule(std::move(connecting));
  { std::lock_guard<std::mutex> lock(adapter->mutex); adapter->state = MUXIVA_RTC_CONNECTED; }
  Event connected; connected.kind = Event::State; connected.a = MUXIVA_RTC_CONNECTED; adapter->schedule(std::move(connected));
  Event participant; participant.kind = Event::Participant; participant.a = MUXIVA_RTC_PARTICIPANT_JOINED; participant.text.assign(request->participant_id.data, request->participant_id.len); adapter->schedule(std::move(participant)); return MUXIVA_STATUS_OK;
 } catch (...) { write_error(error, MUXIVA_STATUS_FOREIGN_EXCEPTION, "MUXIVA-RTC-EXCEPTION", "C++ exception caught at connect boundary"); return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
}

extern "C" muxiva_status_v1 muxiva_rtc_adapter_send_audio_v1(muxiva_rtc_adapter_handle_v1* a, const muxiva_audio_packet_v1* p, muxiva_error_v1* e) {
 try { if (!a || !packet_prefix(p) || !valid_view(p->bytes.data, p->bytes.len, a->max_bytes) || !p->sample_rate_hz || !p->channels) { write_error(e, MUXIVA_STATUS_INVALID_ARGUMENT, "MUXIVA-RTC-AUDIO", "invalid audio packet"); return MUXIVA_STATUS_INVALID_ARGUMENT; } { std::lock_guard<std::mutex> lock(a->mutex); if (a->state != MUXIVA_RTC_CONNECTED) return state_error(e, "adapter is not connected"); } Event x; x.kind=Event::Audio; x.a=p->sample_rate_hz; x.b=p->channels; x.c=p->sample_format; x.d=p->layout; x.wide=p->samples_per_channel; x.timestamp_ns=p->timestamp_ns; x.bytes.assign(p->bytes.data,p->bytes.data+p->bytes.len); a->schedule(std::move(x)); return MUXIVA_STATUS_OK; } catch (...) { write_error(e, MUXIVA_STATUS_FOREIGN_EXCEPTION, "MUXIVA-RTC-EXCEPTION", "audio copy failed"); return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
}
extern "C" muxiva_status_v1 muxiva_rtc_adapter_send_video_v1(muxiva_rtc_adapter_handle_v1* a, const muxiva_video_packet_v1* p, muxiva_error_v1* e) {
 try { if (!a || !packet_prefix(p) || !valid_view(p->bytes.data,p->bytes.len,a->max_bytes) || !p->width || !p->height) { write_error(e,MUXIVA_STATUS_INVALID_ARGUMENT,"MUXIVA-RTC-VIDEO","invalid video packet"); return MUXIVA_STATUS_INVALID_ARGUMENT; } { std::lock_guard<std::mutex> lock(a->mutex); if(a->state!=MUXIVA_RTC_CONNECTED) return state_error(e,"adapter is not connected"); } Event x; x.kind=Event::Video; x.a=p->width;x.b=p->height;x.c=p->pixel_format;x.d=p->plane_count;x.timestamp_ns=p->timestamp_ns;x.bytes.assign(p->bytes.data,p->bytes.data+p->bytes.len);a->schedule(std::move(x));return MUXIVA_STATUS_OK; } catch (...) { write_error(e,MUXIVA_STATUS_FOREIGN_EXCEPTION,"MUXIVA-RTC-EXCEPTION","video copy failed");return MUXIVA_STATUS_FOREIGN_EXCEPTION; }
}
extern "C" muxiva_status_v1 muxiva_rtc_adapter_send_text_v1(muxiva_rtc_adapter_handle_v1* a, const muxiva_text_packet_v1* p, muxiva_error_v1* e) {
 try { if(!a || !packet_prefix(p) || !valid_view(p->text.data,p->text.len,a->max_bytes)){write_error(e,MUXIVA_STATUS_INVALID_ARGUMENT,"MUXIVA-RTC-TEXT","invalid text packet");return MUXIVA_STATUS_INVALID_ARGUMENT;} {std::lock_guard<std::mutex> lock(a->mutex);if(a->state!=MUXIVA_RTC_CONNECTED)return state_error(e,"adapter is not connected");} Event x;x.kind=Event::Text;x.timestamp_ns=p->timestamp_ns;x.text.assign(p->text.data,p->text.len);a->schedule(std::move(x));return MUXIVA_STATUS_OK;} catch(...){write_error(e,MUXIVA_STATUS_FOREIGN_EXCEPTION,"MUXIVA-RTC-EXCEPTION","text copy failed");return MUXIVA_STATUS_FOREIGN_EXCEPTION;}
}

extern "C" muxiva_status_v1 muxiva_rtc_adapter_leave_v1(muxiva_rtc_adapter_handle_v1* a, muxiva_error_v1*) {
 try { if(!a)return MUXIVA_STATUS_INVALID_ARGUMENT; {std::lock_guard<std::mutex> lock(a->mutex);if(a->state==MUXIVA_RTC_LEFT||a->state==MUXIVA_RTC_LEAVING)return MUXIVA_STATUS_OK;a->state=MUXIVA_RTC_LEAVING;} a->context->accepting.store(false,std::memory_order_release);(void)muxiva_session_ingress_close_v1(a->context->ingress); {std::lock_guard<std::mutex> lock(a->mutex);a->preserve_late=a->faults.allow_late_callback!=0;if(!a->preserve_late)a->events.clear();a->stop=true;a->state=MUXIVA_RTC_LEFT;a->cv.notify_all();} std::unique_lock<std::mutex> lock(a->context->drain_mutex);const bool drained=a->context->drain_cv.wait_for(lock,std::chrono::milliseconds(a->drain_ms),[&]{return a->context->in_flight.load(std::memory_order_acquire)==0;});return drained?MUXIVA_STATUS_OK:MUXIVA_STATUS_TIMEOUT;} catch(...){return MUXIVA_STATUS_FOREIGN_EXCEPTION;}
}
extern "C" muxiva_status_v1 muxiva_rtc_adapter_get_stats_v1(muxiva_rtc_adapter_handle_v1* a, muxiva_rtc_adapter_stats_v1* out, muxiva_error_v1* e) { try {if(!a||!out){write_error(e,MUXIVA_STATUS_INVALID_ARGUMENT,"MUXIVA-RTC-STATS","null stats target");return MUXIVA_STATUS_INVALID_ARGUMENT;}*out={MUXIVA_ABI_VERSION_V1,sizeof(*out),a->context->accepted.load(),a->context->full.load(),a->context->closed.load(),a->context->invalid.load(),a->context->late.load(),a->context->in_flight.load(),a->context->last_sequence.load(),a->context->callback_thread_hash.load()};return MUXIVA_STATUS_OK;}catch(...){return MUXIVA_STATUS_FOREIGN_EXCEPTION;} }
extern "C" muxiva_status_v1 muxiva_rtc_adapter_release_test_barrier_v1(muxiva_rtc_adapter_handle_v1* a){if(!a)return MUXIVA_STATUS_INVALID_ARGUMENT;{std::lock_guard<std::mutex> lock(a->context->barrier_mutex);a->context->barrier_held=false;}a->context->barrier_cv.notify_all();return MUXIVA_STATUS_OK;}
extern "C" void muxiva_rtc_adapter_destroy_v1(muxiva_rtc_adapter_handle_v1* a) { try {if(!a)return;(void)muxiva_rtc_adapter_leave_v1(a,nullptr);(void)muxiva_rtc_adapter_release_test_barrier_v1(a);{std::lock_guard<std::mutex> lock(a->mutex);a->stop=true;a->events.clear();a->cv.notify_all();}if(a->worker.joinable())a->worker.join();delete a;}catch(...){/* ABI destroy cannot report; never unwind. */} }

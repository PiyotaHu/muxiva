#include "muxiva/mock_rtc.hpp"

#include <atomic>
#include <cassert>
#include <chrono>
#include <cstring>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

using namespace std::chrono_literals;

struct Recording {
  std::mutex mutex;
  std::vector<std::string> media;
  std::vector<uint32_t> states;
  std::atomic<unsigned> participants{0}, errors{0}, interrupts{0}, notifications{0};
  std::thread::id callback_thread;
};

muxiva_status_v1 media(void* raw, const muxiva_frame_view_v1* frame, muxiva_error_v1*) {
  auto& out = *static_cast<Recording*>(raw);
  std::lock_guard<std::mutex> lock(out.mutex);
  out.callback_thread = std::this_thread::get_id();
  if (frame->header.frame_type == MUXIVA_FRAME_TEXT) out.media.emplace_back(frame->payload.text.text.data, frame->payload.text.text.len);
  else if (frame->header.frame_type == MUXIVA_FRAME_AUDIO) out.media.emplace_back(reinterpret_cast<const char*>(frame->payload.audio.bytes.data), frame->payload.audio.bytes.len);
  else out.media.emplace_back(reinterpret_cast<const char*>(frame->payload.video.bytes.data), frame->payload.video.bytes.len);
  return MUXIVA_STATUS_OK;
}
muxiva_status_v1 state(void* raw, const muxiva_rtc_state_v1* value, muxiva_error_v1*) { auto& out=*static_cast<Recording*>(raw); std::lock_guard<std::mutex> lock(out.mutex); out.states.push_back(value->state); return MUXIVA_STATUS_OK; }
muxiva_status_v1 participant(void* raw, const muxiva_rtc_participant_event_v1*, muxiva_error_v1*) { static_cast<Recording*>(raw)->participants++; return MUXIVA_STATUS_OK; }
muxiva_status_v1 error_cb(void* raw, const muxiva_rtc_error_event_v1*, muxiva_error_v1*) { static_cast<Recording*>(raw)->errors++; return MUXIVA_STATUS_OK; }
muxiva_status_v1 interrupt(void* raw, const muxiva_rtc_interrupt_event_v1*, muxiva_error_v1*) { static_cast<Recording*>(raw)->interrupts++; return MUXIVA_STATUS_OK; }
muxiva_status_v1 notification(void* raw, const muxiva_rtc_notification_v1*, muxiva_error_v1*) { static_cast<Recording*>(raw)->notifications++; return MUXIVA_STATUS_OK; }

struct Core {
  muxiva_runtime_v1 runtime{}; muxiva_session_v1 session{}; muxiva_session_ingress_v1 ingress{}; muxiva_error_v1 error{};
  Core(size_t items) { error.abi_version=MUXIVA_ABI_VERSION_V1;error.struct_size=sizeof(error);assert(muxiva_runtime_create_v1(&runtime,&error)==0);assert(muxiva_session_create_v1(runtime,&session,&error)==0);muxiva_ingress_config_v1 c{MUXIVA_ABI_VERSION_V1,sizeof(c),items,4096};assert(muxiva_session_ingress_create_v1(session,&c,&ingress,&error)==0); }
  ~Core(){(void)muxiva_session_ingress_release_v1(ingress);(void)muxiva_session_release_v1(session);(void)muxiva_runtime_release_v1(runtime);}
};

muxiva_rtc_callbacks_v1 callbacks() { return {MUXIVA_ABI_VERSION_V1,sizeof(muxiva_rtc_callbacks_v1),media,state,participant,error_cb,interrupt,notification}; }
muxiva_rtc_adapter_config_v1 config(muxiva_session_ingress_v1 ingress) { muxiva_rtc_adapter_config_v1 c{};c.abi_version=MUXIVA_ABI_VERSION_V1;c.struct_size=sizeof(c);c.ingress=ingress;c.max_packet_bytes=4096;c.callback_drain_timeout_ms=5;c.faults.abi_version=MUXIVA_ABI_VERSION_V1;c.faults.struct_size=sizeof(c.faults);return c; }

template<class Predicate> void eventually(Predicate predicate) { for(int i=0;i<200&&!predicate();++i)std::this_thread::sleep_for(1ms);assert(predicate()); }

void normal_lifecycle_and_copy() {
  Core core(2); Recording recording; auto cb=callbacks(); auto cfg=config(core.ingress); cfg.faults.delay_ms=2;
  muxiva_rtc_adapter_handle_v1* raw=nullptr; assert(muxiva_rtc_adapter_create_v1(&cfg,&cb,&recording,&raw,&core.error)==0); muxiva::MockRtc rtc(raw);
  const char channel[]="room";const char user[]="alice";muxiva_rtc_join_request_v1 join{MUXIVA_ABI_VERSION_V1,sizeof(join),{channel,4},{user,5}};assert(muxiva_rtc_adapter_connect_v1(rtc.get(),&join,&core.error)==0);
  char audio_bytes[]="abcd";muxiva_audio_packet_v1 audio{MUXIVA_ABI_VERSION_V1,sizeof(audio),48000,1,MUXIVA_PCM_U8,MUXIVA_AUDIO_INTERLEAVED,4,1,{reinterpret_cast<uint8_t*>(audio_bytes),4}};assert(muxiva_rtc_adapter_send_audio_v1(rtc.get(),&audio,&core.error)==0);std::memset(audio_bytes,'x',4);
  uint8_t pixels[]={1,2,3,4};muxiva_video_packet_v1 video{MUXIVA_ABI_VERSION_V1,sizeof(video),1,1,1,1,2,{pixels,4}};assert(muxiva_rtc_adapter_send_video_v1(rtc.get(),&video,&core.error)==0);
  char text[]="hello";muxiva_text_packet_v1 packet{MUXIVA_ABI_VERSION_V1,sizeof(packet),3,{text,5}};assert(muxiva_rtc_adapter_send_text_v1(rtc.get(),&packet,&core.error)==0);std::memset(text,'z',5);
  eventually([&]{std::lock_guard<std::mutex> lock(recording.mutex);return recording.media.size()==3;});
  {std::lock_guard<std::mutex> lock(recording.mutex);assert(recording.media[0]=="abcd");assert(recording.media[2]=="hello");assert(recording.callback_thread!=std::this_thread::get_id());}
  muxiva_ingress_stats_v1 stats{};assert(muxiva_session_ingress_stats_v1(core.ingress,&stats,&core.error)==0);assert(stats.accepted==2);assert(stats.full_drops>=1);
  assert(muxiva_rtc_adapter_leave_v1(rtc.get(),&core.error)==0);assert(muxiva_rtc_adapter_leave_v1(rtc.get(),&core.error)==0);assert(muxiva_rtc_adapter_send_text_v1(rtc.get(),&packet,&core.error)==MUXIVA_STATUS_CLOSED);rtc.reset();rtc.reset();
}

void deterministic_loss_and_reorder() {
  Core core(16); Recording recording; auto cb=callbacks();auto cfg=config(core.ingress);cfg.faults.drop_every_n=2;cfg.faults.reorder_window=3;
  muxiva_rtc_adapter_handle_v1* raw=nullptr;assert(muxiva_rtc_adapter_create_v1(&cfg,&cb,&recording,&raw,&core.error)==0);muxiva::MockRtc rtc(raw);const char x[]="x";muxiva_rtc_join_request_v1 join{MUXIVA_ABI_VERSION_V1,sizeof(join),{x,1},{x,1}};assert(muxiva_rtc_adapter_connect_v1(rtc.get(),&join,&core.error)==0);
  muxiva_text_packet_v1 packet{MUXIVA_ABI_VERSION_V1,sizeof(packet),0,{x,1}};assert(muxiva_rtc_adapter_send_text_v1(rtc.get(),&packet,&core.error)==0);
  for(char ch='a';ch<='f';++ch){muxiva_text_packet_v1 p{MUXIVA_ABI_VERSION_V1,sizeof(p),0,{&ch,1}};assert(muxiva_rtc_adapter_send_text_v1(rtc.get(),&p,&core.error)==0);} eventually([&]{muxiva_rtc_adapter_stats_v1 s{};muxiva_rtc_adapter_get_stats_v1(rtc.get(),&s,&core.error);return s.last_sequence>=7;});assert(muxiva_rtc_adapter_leave_v1(rtc.get(),&core.error)==0);
}

void drain_timeout_keeps_context_alive() {
  Core core(4); Recording recording;auto cb=callbacks();auto cfg=config(core.ingress);cfg.faults.hold_callback_entry=1;cfg.callback_drain_timeout_ms=1;
  muxiva_rtc_adapter_handle_v1* raw=nullptr;assert(muxiva_rtc_adapter_create_v1(&cfg,&cb,&recording,&raw,&core.error)==0);muxiva::MockRtc rtc(raw);const char x[]="x";muxiva_rtc_join_request_v1 join{MUXIVA_ABI_VERSION_V1,sizeof(join),{x,1},{x,1}};assert(muxiva_rtc_adapter_connect_v1(rtc.get(),&join,&core.error)==0);
  eventually([&]{muxiva_rtc_adapter_stats_v1 s{};muxiva_rtc_adapter_get_stats_v1(rtc.get(),&s,&core.error);return s.last_sequence>=3;});
  muxiva_text_packet_v1 packet{MUXIVA_ABI_VERSION_V1,sizeof(packet),0,{x,1}};assert(muxiva_rtc_adapter_send_text_v1(rtc.get(),&packet,&core.error)==0);
  eventually([&]{muxiva_rtc_adapter_stats_v1 s{};muxiva_rtc_adapter_get_stats_v1(rtc.get(),&s,&core.error);return s.in_flight==1;});assert(muxiva_rtc_adapter_leave_v1(rtc.get(),&core.error)==MUXIVA_STATUS_TIMEOUT);assert(muxiva_rtc_adapter_release_test_barrier_v1(rtc.get())==0);eventually([&]{muxiva_rtc_adapter_stats_v1 s{};muxiva_rtc_adapter_get_stats_v1(rtc.get(),&s,&core.error);return s.in_flight==0&&s.late_dropped>=1;});
}

void concurrent_leave_is_idempotent() {
  Core core(8); Recording recording; auto cb=callbacks(); auto cfg=config(core.ingress);
  muxiva_rtc_adapter_handle_v1* raw=nullptr;assert(muxiva_rtc_adapter_create_v1(&cfg,&cb,&recording,&raw,&core.error)==0);muxiva::MockRtc rtc(raw);const char x[]="x";muxiva_rtc_join_request_v1 join{MUXIVA_ABI_VERSION_V1,sizeof(join),{x,1},{x,1}};assert(muxiva_rtc_adapter_connect_v1(rtc.get(),&join,&core.error)==0);
  std::vector<std::thread> callers;std::atomic<unsigned> ok{0};for(int i=0;i<8;++i)callers.emplace_back([&]{muxiva_error_v1 e{};if(muxiva_rtc_adapter_leave_v1(rtc.get(),&e)==MUXIVA_STATUS_OK)++ok;});for(auto& thread:callers)thread.join();assert(ok==8);
}

int main(){
  muxiva_error_v1 error{};muxiva_rtc_adapter_handle_v1* raw=nullptr;muxiva_rtc_adapter_config_v1 bad{};muxiva_rtc_callbacks_v1 cb{};assert(muxiva_rtc_adapter_create_v1(&bad,&cb,nullptr,&raw,&error)==MUXIVA_STATUS_INVALID_ARGUMENT);
  normal_lifecycle_and_copy();deterministic_loss_and_reorder();drain_timeout_keeps_context_alive();concurrent_leave_is_idempotent();
}

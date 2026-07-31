#include "voxa/mock_rtc.hpp"

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

voxa_status_v1 media(void* raw, const voxa_frame_view_v1* frame, voxa_error_v1*) {
  auto& out = *static_cast<Recording*>(raw);
  std::lock_guard<std::mutex> lock(out.mutex);
  out.callback_thread = std::this_thread::get_id();
  if (frame->header.frame_type == VOXA_FRAME_TEXT) out.media.emplace_back(frame->payload.text.text.data, frame->payload.text.text.len);
  else if (frame->header.frame_type == VOXA_FRAME_AUDIO) out.media.emplace_back(reinterpret_cast<const char*>(frame->payload.audio.bytes.data), frame->payload.audio.bytes.len);
  else out.media.emplace_back(reinterpret_cast<const char*>(frame->payload.video.bytes.data), frame->payload.video.bytes.len);
  return VOXA_STATUS_OK;
}
voxa_status_v1 state(void* raw, const voxa_rtc_state_v1* value, voxa_error_v1*) { auto& out=*static_cast<Recording*>(raw); std::lock_guard<std::mutex> lock(out.mutex); out.states.push_back(value->state); return VOXA_STATUS_OK; }
voxa_status_v1 participant(void* raw, const voxa_rtc_participant_event_v1*, voxa_error_v1*) { static_cast<Recording*>(raw)->participants++; return VOXA_STATUS_OK; }
voxa_status_v1 error_cb(void* raw, const voxa_rtc_error_event_v1*, voxa_error_v1*) { static_cast<Recording*>(raw)->errors++; return VOXA_STATUS_OK; }
voxa_status_v1 interrupt(void* raw, const voxa_rtc_interrupt_event_v1*, voxa_error_v1*) { static_cast<Recording*>(raw)->interrupts++; return VOXA_STATUS_OK; }
voxa_status_v1 notification(void* raw, const voxa_rtc_notification_v1*, voxa_error_v1*) { static_cast<Recording*>(raw)->notifications++; return VOXA_STATUS_OK; }

struct Core {
  voxa_runtime_v1 runtime{}; voxa_session_v1 session{}; voxa_session_ingress_v1 ingress{}; voxa_error_v1 error{};
  Core(size_t items) { error.abi_version=VOXA_ABI_VERSION_V1;error.struct_size=sizeof(error);assert(voxa_runtime_create_v1(&runtime,&error)==0);assert(voxa_session_create_v1(runtime,&session,&error)==0);voxa_ingress_config_v1 c{VOXA_ABI_VERSION_V1,sizeof(c),items,4096};assert(voxa_session_ingress_create_v1(session,&c,&ingress,&error)==0); }
  ~Core(){(void)voxa_session_ingress_release_v1(ingress);(void)voxa_session_release_v1(session);(void)voxa_runtime_release_v1(runtime);}
};

voxa_rtc_callbacks_v1 callbacks() { return {VOXA_ABI_VERSION_V1,sizeof(voxa_rtc_callbacks_v1),media,state,participant,error_cb,interrupt,notification}; }
voxa_rtc_adapter_config_v1 config(voxa_session_ingress_v1 ingress) { voxa_rtc_adapter_config_v1 c{};c.abi_version=VOXA_ABI_VERSION_V1;c.struct_size=sizeof(c);c.ingress=ingress;c.max_packet_bytes=4096;c.callback_drain_timeout_ms=5;c.faults.abi_version=VOXA_ABI_VERSION_V1;c.faults.struct_size=sizeof(c.faults);return c; }

template<class Predicate> void eventually(Predicate predicate) { for(int i=0;i<200&&!predicate();++i)std::this_thread::sleep_for(1ms);assert(predicate()); }

void normal_lifecycle_and_copy() {
  Core core(2); Recording recording; auto cb=callbacks(); auto cfg=config(core.ingress); cfg.faults.delay_ms=2;
  voxa_rtc_adapter_handle_v1* raw=nullptr; assert(voxa_rtc_adapter_create_v1(&cfg,&cb,&recording,&raw,&core.error)==0); voxa::MockRtc rtc(raw);
  const char channel[]="room";const char user[]="alice";voxa_rtc_join_request_v1 join{VOXA_ABI_VERSION_V1,sizeof(join),{channel,4},{user,5}};assert(voxa_rtc_adapter_connect_v1(rtc.get(),&join,&core.error)==0);
  char audio_bytes[]="abcd";voxa_audio_packet_v1 audio{VOXA_ABI_VERSION_V1,sizeof(audio),48000,1,VOXA_PCM_U8,VOXA_AUDIO_INTERLEAVED,4,1,{reinterpret_cast<uint8_t*>(audio_bytes),4}};assert(voxa_rtc_adapter_send_audio_v1(rtc.get(),&audio,&core.error)==0);std::memset(audio_bytes,'x',4);
  uint8_t pixels[]={1,2,3,4};voxa_video_packet_v1 video{VOXA_ABI_VERSION_V1,sizeof(video),1,1,1,1,2,{pixels,4}};assert(voxa_rtc_adapter_send_video_v1(rtc.get(),&video,&core.error)==0);
  char text[]="hello";voxa_text_packet_v1 packet{VOXA_ABI_VERSION_V1,sizeof(packet),3,{text,5}};assert(voxa_rtc_adapter_send_text_v1(rtc.get(),&packet,&core.error)==0);std::memset(text,'z',5);
  eventually([&]{std::lock_guard<std::mutex> lock(recording.mutex);return recording.media.size()==3;});
  {std::lock_guard<std::mutex> lock(recording.mutex);assert(recording.media[0]=="abcd");assert(recording.media[2]=="hello");assert(recording.callback_thread!=std::this_thread::get_id());}
  voxa_ingress_stats_v1 stats{};assert(voxa_session_ingress_stats_v1(core.ingress,&stats,&core.error)==0);assert(stats.accepted==2);assert(stats.full_drops>=1);
  assert(voxa_rtc_adapter_leave_v1(rtc.get(),&core.error)==0);assert(voxa_rtc_adapter_leave_v1(rtc.get(),&core.error)==0);assert(voxa_rtc_adapter_send_text_v1(rtc.get(),&packet,&core.error)==VOXA_STATUS_CLOSED);rtc.reset();rtc.reset();
}

void deterministic_loss_and_reorder() {
  Core core(16); Recording recording; auto cb=callbacks();auto cfg=config(core.ingress);cfg.faults.drop_every_n=2;cfg.faults.reorder_window=3;
  voxa_rtc_adapter_handle_v1* raw=nullptr;assert(voxa_rtc_adapter_create_v1(&cfg,&cb,&recording,&raw,&core.error)==0);voxa::MockRtc rtc(raw);const char x[]="x";voxa_rtc_join_request_v1 join{VOXA_ABI_VERSION_V1,sizeof(join),{x,1},{x,1}};assert(voxa_rtc_adapter_connect_v1(rtc.get(),&join,&core.error)==0);
  voxa_text_packet_v1 packet{VOXA_ABI_VERSION_V1,sizeof(packet),0,{x,1}};assert(voxa_rtc_adapter_send_text_v1(rtc.get(),&packet,&core.error)==0);
  for(char ch='a';ch<='f';++ch){voxa_text_packet_v1 p{VOXA_ABI_VERSION_V1,sizeof(p),0,{&ch,1}};assert(voxa_rtc_adapter_send_text_v1(rtc.get(),&p,&core.error)==0);} eventually([&]{voxa_rtc_adapter_stats_v1 s{};voxa_rtc_adapter_get_stats_v1(rtc.get(),&s,&core.error);return s.last_sequence>=7;});assert(voxa_rtc_adapter_leave_v1(rtc.get(),&core.error)==0);
}

void drain_timeout_keeps_context_alive() {
  Core core(4); Recording recording;auto cb=callbacks();auto cfg=config(core.ingress);cfg.faults.hold_callback_entry=1;cfg.callback_drain_timeout_ms=1;
  voxa_rtc_adapter_handle_v1* raw=nullptr;assert(voxa_rtc_adapter_create_v1(&cfg,&cb,&recording,&raw,&core.error)==0);voxa::MockRtc rtc(raw);const char x[]="x";voxa_rtc_join_request_v1 join{VOXA_ABI_VERSION_V1,sizeof(join),{x,1},{x,1}};assert(voxa_rtc_adapter_connect_v1(rtc.get(),&join,&core.error)==0);
  eventually([&]{voxa_rtc_adapter_stats_v1 s{};voxa_rtc_adapter_get_stats_v1(rtc.get(),&s,&core.error);return s.last_sequence>=3;});
  voxa_text_packet_v1 packet{VOXA_ABI_VERSION_V1,sizeof(packet),0,{x,1}};assert(voxa_rtc_adapter_send_text_v1(rtc.get(),&packet,&core.error)==0);
  eventually([&]{voxa_rtc_adapter_stats_v1 s{};voxa_rtc_adapter_get_stats_v1(rtc.get(),&s,&core.error);return s.in_flight==1;});assert(voxa_rtc_adapter_leave_v1(rtc.get(),&core.error)==VOXA_STATUS_TIMEOUT);assert(voxa_rtc_adapter_release_test_barrier_v1(rtc.get())==0);eventually([&]{voxa_rtc_adapter_stats_v1 s{};voxa_rtc_adapter_get_stats_v1(rtc.get(),&s,&core.error);return s.in_flight==0&&s.late_dropped>=1;});
}

void concurrent_leave_is_idempotent() {
  Core core(8); Recording recording; auto cb=callbacks(); auto cfg=config(core.ingress);
  voxa_rtc_adapter_handle_v1* raw=nullptr;assert(voxa_rtc_adapter_create_v1(&cfg,&cb,&recording,&raw,&core.error)==0);voxa::MockRtc rtc(raw);const char x[]="x";voxa_rtc_join_request_v1 join{VOXA_ABI_VERSION_V1,sizeof(join),{x,1},{x,1}};assert(voxa_rtc_adapter_connect_v1(rtc.get(),&join,&core.error)==0);
  std::vector<std::thread> callers;std::atomic<unsigned> ok{0};for(int i=0;i<8;++i)callers.emplace_back([&]{voxa_error_v1 e{};if(voxa_rtc_adapter_leave_v1(rtc.get(),&e)==VOXA_STATUS_OK)++ok;});for(auto& thread:callers)thread.join();assert(ok==8);
}

int main(){
  voxa_error_v1 error{};voxa_rtc_adapter_handle_v1* raw=nullptr;voxa_rtc_adapter_config_v1 bad{};voxa_rtc_callbacks_v1 cb{};assert(voxa_rtc_adapter_create_v1(&bad,&cb,nullptr,&raw,&error)==VOXA_STATUS_INVALID_ARGUMENT);
  normal_lifecycle_and_copy();deterministic_loss_and_reorder();drain_timeout_keeps_context_alive();concurrent_leave_is_idempotent();
}

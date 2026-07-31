#ifndef VOXA_RTC_ADAPTER_V1_H
#define VOXA_RTC_ADAPTER_V1_H

#include "voxa/voxa.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct voxa_rtc_adapter_handle_v1 voxa_rtc_adapter_handle_v1;

enum { VOXA_RTC_CREATED = 1, VOXA_RTC_CONNECTING = 2, VOXA_RTC_CONNECTED = 3,
       VOXA_RTC_LEAVING = 4, VOXA_RTC_LEFT = 5, VOXA_RTC_FAILED = 6 };
enum { VOXA_RTC_PARTICIPANT_JOINED = 1, VOXA_RTC_PARTICIPANT_LEFT = 2 };

typedef struct voxa_rtc_state_v1 { uint32_t abi_version; uint32_t struct_size; uint32_t state; uint32_t reserved; uint64_t sequence; } voxa_rtc_state_v1;
typedef struct voxa_rtc_participant_event_v1 { uint32_t abi_version; uint32_t struct_size; uint32_t kind; uint32_t reserved; voxa_str_v1 participant_id; uint64_t sequence; } voxa_rtc_participant_event_v1;
typedef struct voxa_rtc_error_event_v1 { uint32_t abi_version; uint32_t struct_size; int32_t code; uint32_t reserved; voxa_str_v1 message; uint64_t sequence; } voxa_rtc_error_event_v1;
typedef struct voxa_rtc_interrupt_event_v1 { uint32_t abi_version; uint32_t struct_size; voxa_str_v1 name; voxa_bytes_v1 payload; uint64_t sequence; } voxa_rtc_interrupt_event_v1;
typedef struct voxa_rtc_notification_v1 { uint32_t abi_version; uint32_t struct_size; voxa_str_v1 topic; voxa_bytes_v1 payload; uint64_t sequence; } voxa_rtc_notification_v1;

typedef struct voxa_rtc_callbacks_v1 {
  uint32_t abi_version; uint32_t struct_size;
  voxa_status_v1 (*on_media)(void *, const voxa_frame_view_v1 *, voxa_error_v1 *);
  voxa_status_v1 (*on_connection_state)(void *, const voxa_rtc_state_v1 *, voxa_error_v1 *);
  voxa_status_v1 (*on_participant)(void *, const voxa_rtc_participant_event_v1 *, voxa_error_v1 *);
  voxa_status_v1 (*on_error)(void *, const voxa_rtc_error_event_v1 *, voxa_error_v1 *);
  voxa_status_v1 (*on_interrupt)(void *, const voxa_rtc_interrupt_event_v1 *, voxa_error_v1 *);
  voxa_status_v1 (*on_notification)(void *, const voxa_rtc_notification_v1 *, voxa_error_v1 *);
} voxa_rtc_callbacks_v1;

typedef struct voxa_rtc_mock_faults_v1 {
  uint32_t abi_version; uint32_t struct_size;
  uint32_t delay_ms; uint32_t drop_every_n; uint32_t reorder_window;
  uint32_t disconnect_after_n; uint32_t allow_late_callback; uint32_t hold_callback_entry;
  uint64_t seed;
} voxa_rtc_mock_faults_v1;

typedef struct voxa_rtc_adapter_config_v1 {
  uint32_t abi_version; uint32_t struct_size;
  voxa_session_ingress_v1 ingress;
  size_t max_packet_bytes;
  uint32_t callback_drain_timeout_ms; uint32_t reserved;
  voxa_rtc_mock_faults_v1 faults;
} voxa_rtc_adapter_config_v1;

typedef struct voxa_rtc_join_request_v1 { uint32_t abi_version; uint32_t struct_size; voxa_str_v1 channel; voxa_str_v1 participant_id; } voxa_rtc_join_request_v1;
typedef struct voxa_audio_packet_v1 { uint32_t abi_version; uint32_t struct_size; uint32_t sample_rate_hz; uint16_t channels; uint16_t sample_format; uint32_t layout; uint64_t samples_per_channel; int64_t timestamp_ns; voxa_bytes_v1 bytes; } voxa_audio_packet_v1;
typedef struct voxa_video_packet_v1 { uint32_t abi_version; uint32_t struct_size; uint32_t width; uint32_t height; uint32_t pixel_format; uint32_t plane_count; int64_t timestamp_ns; voxa_bytes_v1 bytes; } voxa_video_packet_v1;
typedef struct voxa_text_packet_v1 { uint32_t abi_version; uint32_t struct_size; int64_t timestamp_ns; voxa_str_v1 text; } voxa_text_packet_v1;

typedef struct voxa_rtc_adapter_stats_v1 {
  uint32_t abi_version; uint32_t struct_size;
  uint64_t accepted; uint64_t full_dropped; uint64_t closed_dropped;
  uint64_t invalid; uint64_t late_dropped; uint64_t in_flight;
  uint64_t last_sequence; uint64_t callback_thread_hash;
} voxa_rtc_adapter_stats_v1;

voxa_status_v1 voxa_rtc_adapter_create_v1(const voxa_rtc_adapter_config_v1 *, const voxa_rtc_callbacks_v1 *, void *, voxa_rtc_adapter_handle_v1 **, voxa_error_v1 *);
voxa_status_v1 voxa_rtc_adapter_connect_v1(voxa_rtc_adapter_handle_v1 *, const voxa_rtc_join_request_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_rtc_adapter_send_audio_v1(voxa_rtc_adapter_handle_v1 *, const voxa_audio_packet_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_rtc_adapter_send_video_v1(voxa_rtc_adapter_handle_v1 *, const voxa_video_packet_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_rtc_adapter_send_text_v1(voxa_rtc_adapter_handle_v1 *, const voxa_text_packet_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_rtc_adapter_leave_v1(voxa_rtc_adapter_handle_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_rtc_adapter_get_stats_v1(voxa_rtc_adapter_handle_v1 *, voxa_rtc_adapter_stats_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_rtc_adapter_release_test_barrier_v1(voxa_rtc_adapter_handle_v1 *);
void voxa_rtc_adapter_destroy_v1(voxa_rtc_adapter_handle_v1 *);

#ifdef __cplusplus
}
#endif
#endif

#ifndef VOXA_VOXA_H
#define VOXA_VOXA_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define VOXA_ABI_VERSION_V1 UINT32_C(0x00010000)
#define VOXA_CAP_COPY_INGRESS (UINT64_C(1) << 0)
#define VOXA_CAP_RETAIN_RELEASE (UINT64_C(1) << 1) /* reserved; capability is clear in v1 */

typedef int32_t voxa_status_v1;
enum {
  VOXA_STATUS_OK = 0,
  VOXA_STATUS_INVALID_ARGUMENT = 1,
  VOXA_STATUS_ABI_MISMATCH = 2,
  VOXA_STATUS_INVALID_HANDLE = 3,
  VOXA_STATUS_CLOSED = 4,
  VOXA_STATUS_BUSY = 5,
  VOXA_STATUS_QUEUE_FULL = 6,
  VOXA_STATUS_CANCELLED = 7,
  VOXA_STATUS_TIMEOUT = 8,
  VOXA_STATUS_UNSUPPORTED = 9,
  VOXA_STATUS_EXTERNAL = 10,
  VOXA_STATUS_FOREIGN_EXCEPTION = 11,
  VOXA_STATUS_INTERNAL = 12,
  VOXA_STATUS_PANIC = 13
};

enum {
  VOXA_ERROR_CATEGORY_VALIDATION = 1,
  VOXA_ERROR_CATEGORY_LIFECYCLE = 2,
  VOXA_ERROR_CATEGORY_CANCELLED = 3,
  VOXA_ERROR_CATEGORY_EXTERNAL = 4,
  VOXA_ERROR_CATEGORY_FOREIGN_EXCEPTION = 5,
  VOXA_ERROR_CATEGORY_INTERNAL = 6
};

typedef struct voxa_token_v1 { uint64_t slot; uint64_t generation; } voxa_token_v1;
typedef voxa_token_v1 voxa_runtime_v1;
typedef voxa_token_v1 voxa_session_v1;
typedef voxa_token_v1 voxa_frame_v1;
typedef voxa_token_v1 voxa_node_v1;
typedef voxa_token_v1 voxa_session_ingress_v1;

typedef struct voxa_str_v1 { const char *data; size_t len; } voxa_str_v1;
typedef struct voxa_bytes_v1 { const uint8_t *data; size_t len; } voxa_bytes_v1;

typedef struct voxa_error_v1 {
  uint32_t abi_version;
  uint32_t struct_size;
  int32_t status;
  int32_t category;
  char code[64];
  char message[256];
} voxa_error_v1;

enum {
  VOXA_FRAME_AUDIO = 1, VOXA_FRAME_VIDEO = 2, VOXA_FRAME_TEXT = 3,
  VOXA_FRAME_BYTE = 4, VOXA_FRAME_SIGNAL = 5, VOXA_FRAME_EVENT = 6
};
enum { VOXA_CLOCK_MONOTONIC = 1, VOXA_CLOCK_MEDIA_RELATIVE = 2, VOXA_CLOCK_WALL = 3 };
enum { VOXA_PCM_U8 = 1, VOXA_PCM_I16LE = 2, VOXA_PCM_I24LE = 3, VOXA_PCM_I32LE = 4,
       VOXA_PCM_F32LE = 5, VOXA_PCM_F64LE = 6 };
enum { VOXA_AUDIO_INTERLEAVED = 1, VOXA_AUDIO_PLANAR = 2 };

typedef struct voxa_frame_header_v1 {
  uint32_t abi_version;
  uint32_t struct_size;
  uint32_t frame_type;
  uint32_t clock_kind;
  int64_t timestamp_ns;
  uint64_t sequence_id;
  voxa_str_v1 frame_id;
  voxa_str_v1 clock_domain_id;
  voxa_str_v1 stream_id;
  voxa_str_v1 trace_id;
  uint64_t reserved[4];
} voxa_frame_header_v1;

typedef struct voxa_audio_payload_v1 {
  uint32_t sample_rate_hz;
  uint16_t channels;
  uint16_t sample_format;
  uint32_t layout;
  uint32_t reserved0;
  uint64_t samples_per_channel;
  voxa_bytes_v1 bytes;
  uint64_t reserved[2];
} voxa_audio_payload_v1;
typedef struct voxa_video_payload_v1 {
  uint32_t width; uint32_t height; uint32_t pixel_format; uint32_t plane_count;
  voxa_bytes_v1 bytes; uint64_t reserved[4];
} voxa_video_payload_v1;
typedef struct voxa_text_payload_v1 { voxa_str_v1 text; voxa_str_v1 media_type; uint64_t reserved[2]; } voxa_text_payload_v1;
typedef struct voxa_byte_payload_v1 { voxa_bytes_v1 bytes; voxa_str_v1 media_type; uint64_t reserved[2]; } voxa_byte_payload_v1;
typedef struct voxa_signal_payload_v1 { voxa_str_v1 signal_name; voxa_str_v1 source_node_id; voxa_bytes_v1 value; uint64_t reserved[2]; } voxa_signal_payload_v1;
typedef struct voxa_event_payload_v1 { voxa_str_v1 topic; voxa_bytes_v1 value; uint64_t reserved[2]; } voxa_event_payload_v1;

typedef union voxa_frame_payload_v1 {
  voxa_audio_payload_v1 audio; voxa_video_payload_v1 video;
  voxa_text_payload_v1 text; voxa_byte_payload_v1 bytes;
  voxa_signal_payload_v1 signal; voxa_event_payload_v1 event;
} voxa_frame_payload_v1;
typedef struct voxa_frame_view_v1 { voxa_frame_header_v1 header; voxa_frame_payload_v1 payload; } voxa_frame_view_v1;

typedef struct voxa_abort_reason_v1 {
  uint32_t abi_version; uint32_t struct_size; int32_t category; int32_t stage;
  voxa_str_v1 code; voxa_str_v1 message;
} voxa_abort_reason_v1;

typedef voxa_status_v1 (*voxa_node_simple_fn_v1)(void *, voxa_error_v1 *);
typedef voxa_status_v1 (*voxa_node_process_fn_v1)(void *, const voxa_frame_view_v1 *,
                                                  voxa_frame_view_v1 *, voxa_error_v1 *);
typedef voxa_status_v1 (*voxa_node_signal_fn_v1)(void *, const voxa_frame_view_v1 *, voxa_error_v1 *);
typedef void (*voxa_node_abort_fn_v1)(void *, const voxa_abort_reason_v1 *) ;
typedef void (*voxa_node_destroy_fn_v1)(void *);

typedef struct voxa_node_vtable_v1 {
  uint32_t abi_version; uint32_t struct_size; void *user_data;
  voxa_node_simple_fn_v1 on_prepare;
  voxa_node_process_fn_v1 on_process;
  voxa_node_signal_fn_v1 on_signal;
  voxa_node_simple_fn_v1 on_finish;
  voxa_node_abort_fn_v1 on_abort;
  voxa_node_destroy_fn_v1 destroy;
  uint64_t capabilities;
  uint64_t reserved[3];
} voxa_node_vtable_v1;

uint32_t voxa_abi_version_v1(void);
uint64_t voxa_capabilities_v1(void);
voxa_status_v1 voxa_runtime_create_v1(voxa_runtime_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_runtime_release_v1(voxa_runtime_v1);
voxa_status_v1 voxa_session_create_v1(voxa_runtime_v1, voxa_session_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_session_release_v1(voxa_session_v1);
typedef struct voxa_ingress_config_v1 { uint32_t abi_version; uint32_t struct_size; size_t item_capacity; size_t byte_capacity; } voxa_ingress_config_v1;
typedef struct voxa_ingress_stats_v1 { uint32_t abi_version; uint32_t struct_size; uint64_t accepted; uint64_t full_drops; uint64_t closed_drops; size_t queued_items; size_t queued_bytes; } voxa_ingress_stats_v1;
voxa_status_v1 voxa_session_ingress_create_v1(voxa_session_v1, const voxa_ingress_config_v1 *, voxa_session_ingress_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_session_ingress_clone_v1(voxa_session_ingress_v1, voxa_session_ingress_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_session_ingress_close_v1(voxa_session_ingress_v1);
voxa_status_v1 voxa_session_ingress_release_v1(voxa_session_ingress_v1);
voxa_status_v1 voxa_session_ingress_try_submit_v1(voxa_session_ingress_v1, const voxa_frame_view_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_session_ingress_stats_v1(voxa_session_ingress_v1, voxa_ingress_stats_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_session_ingress_try_pop_v1(voxa_session_ingress_v1, voxa_frame_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_frame_copy_v1(const voxa_frame_view_v1 *, voxa_frame_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_frame_release_v1(voxa_frame_v1);
voxa_status_v1 voxa_frame_retain_v1(voxa_frame_v1); /* always UNSUPPORTED in v1 */
voxa_status_v1 voxa_node_create_v1(const voxa_node_vtable_v1 *, voxa_node_v1 *, voxa_error_v1 *);
voxa_status_v1 voxa_node_release_v1(voxa_node_v1);

/* Focused Stage-7 graph harness: source -> foreign transform -> Rust sink. */
voxa_status_v1 voxa_runtime_run_text_v1(voxa_runtime_v1, voxa_node_v1,
                                         const voxa_frame_view_v1 *,
                                         char *output, size_t output_capacity,
                                         size_t *output_len, voxa_error_v1 *);

#ifdef __cplusplus
}
#endif

#if defined(__cplusplus)
static_assert(sizeof(voxa_token_v1) == 16, "voxa token ABI drift");
#else
_Static_assert(sizeof(voxa_token_v1) == 16, "voxa token ABI drift");
#endif

#endif

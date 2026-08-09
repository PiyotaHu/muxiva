#ifndef MUXIVA_MUXIVA_H
#define MUXIVA_MUXIVA_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define MUXIVA_ABI_VERSION_V1 UINT32_C(0x00010000)
#define MUXIVA_CAP_COPY_INGRESS (UINT64_C(1) << 0)
#define MUXIVA_CAP_RETAIN_RELEASE (UINT64_C(1) << 1) /* reserved; capability is clear in v1 */
#define MUXIVA_CAP_GRAPH_FACTORIES (UINT64_C(1) << 2)

typedef int32_t muxiva_status_v1;
enum {
  MUXIVA_STATUS_OK = 0,
  MUXIVA_STATUS_INVALID_ARGUMENT = 1,
  MUXIVA_STATUS_ABI_MISMATCH = 2,
  MUXIVA_STATUS_INVALID_HANDLE = 3,
  MUXIVA_STATUS_CLOSED = 4,
  MUXIVA_STATUS_BUSY = 5,
  MUXIVA_STATUS_QUEUE_FULL = 6,
  MUXIVA_STATUS_CANCELLED = 7,
  MUXIVA_STATUS_TIMEOUT = 8,
  MUXIVA_STATUS_UNSUPPORTED = 9,
  MUXIVA_STATUS_EXTERNAL = 10,
  MUXIVA_STATUS_FOREIGN_EXCEPTION = 11,
  MUXIVA_STATUS_INTERNAL = 12,
  MUXIVA_STATUS_PANIC = 13
};

enum {
  MUXIVA_ERROR_CATEGORY_VALIDATION = 1,
  MUXIVA_ERROR_CATEGORY_LIFECYCLE = 2,
  MUXIVA_ERROR_CATEGORY_CANCELLED = 3,
  MUXIVA_ERROR_CATEGORY_EXTERNAL = 4,
  MUXIVA_ERROR_CATEGORY_FOREIGN_EXCEPTION = 5,
  MUXIVA_ERROR_CATEGORY_INTERNAL = 6
};

typedef struct muxiva_token_v1 { uint64_t slot; uint64_t generation; } muxiva_token_v1;
typedef muxiva_token_v1 muxiva_runtime_v1;
typedef muxiva_token_v1 muxiva_session_v1;
typedef muxiva_token_v1 muxiva_frame_v1;
typedef muxiva_token_v1 muxiva_node_v1;
typedef muxiva_token_v1 muxiva_session_ingress_v1;

typedef struct muxiva_str_v1 { const char *data; size_t len; } muxiva_str_v1;
typedef struct muxiva_bytes_v1 { const uint8_t *data; size_t len; } muxiva_bytes_v1;

typedef struct muxiva_error_v1 {
  uint32_t abi_version;
  uint32_t struct_size;
  int32_t status;
  int32_t category;
  char code[64];
  char message[256];
} muxiva_error_v1;

enum {
  MUXIVA_FRAME_AUDIO = 1, MUXIVA_FRAME_VIDEO = 2, MUXIVA_FRAME_TEXT = 3,
  MUXIVA_FRAME_BYTE = 4, MUXIVA_FRAME_SIGNAL = 5, MUXIVA_FRAME_EVENT = 6
};
enum { MUXIVA_CLOCK_MONOTONIC = 1, MUXIVA_CLOCK_MEDIA_RELATIVE = 2, MUXIVA_CLOCK_WALL = 3 };
enum { MUXIVA_PCM_U8 = 1, MUXIVA_PCM_I16LE = 2, MUXIVA_PCM_I24LE = 3, MUXIVA_PCM_I32LE = 4,
       MUXIVA_PCM_F32LE = 5, MUXIVA_PCM_F64LE = 6 };
enum { MUXIVA_AUDIO_INTERLEAVED = 1, MUXIVA_AUDIO_PLANAR = 2 };
enum { MUXIVA_PIXEL_RGBA8 = 1, MUXIVA_PIXEL_I420 = 2 };
enum { MUXIVA_NODE_SOURCE = 1, MUXIVA_NODE_TRANSFORM = 2, MUXIVA_NODE_SINK = 3 };

typedef struct muxiva_frame_header_v1 {
  uint32_t abi_version;
  uint32_t struct_size;
  uint32_t frame_type;
  uint32_t clock_kind;
  int64_t timestamp_ns;
  uint64_t sequence_id;
  muxiva_str_v1 frame_id;
  muxiva_str_v1 clock_domain_id;
  muxiva_str_v1 stream_id;
  muxiva_str_v1 trace_id;
  uint64_t reserved[4];
} muxiva_frame_header_v1;

typedef struct muxiva_audio_payload_v1 {
  uint32_t sample_rate_hz;
  uint16_t channels;
  uint16_t sample_format;
  uint32_t layout;
  uint32_t reserved0;
  uint64_t samples_per_channel;
  muxiva_bytes_v1 bytes;
  uint64_t reserved[2];
} muxiva_audio_payload_v1;
typedef struct muxiva_video_payload_v1 {
  uint32_t width; uint32_t height; uint32_t pixel_format; uint32_t plane_count;
  muxiva_bytes_v1 bytes; uint64_t reserved[4];
} muxiva_video_payload_v1;
typedef struct muxiva_text_payload_v1 { muxiva_str_v1 text; muxiva_str_v1 media_type; uint64_t reserved[2]; } muxiva_text_payload_v1;
typedef struct muxiva_byte_payload_v1 { muxiva_bytes_v1 bytes; muxiva_str_v1 media_type; uint64_t reserved[2]; } muxiva_byte_payload_v1;
typedef struct muxiva_signal_payload_v1 { muxiva_str_v1 signal_name; muxiva_str_v1 source_node_id; muxiva_bytes_v1 value; uint64_t reserved[2]; } muxiva_signal_payload_v1;
typedef struct muxiva_event_payload_v1 { muxiva_str_v1 topic; muxiva_bytes_v1 value; uint64_t reserved[2]; } muxiva_event_payload_v1;

typedef union muxiva_frame_payload_v1 {
  muxiva_audio_payload_v1 audio; muxiva_video_payload_v1 video;
  muxiva_text_payload_v1 text; muxiva_byte_payload_v1 bytes;
  muxiva_signal_payload_v1 signal; muxiva_event_payload_v1 event;
} muxiva_frame_payload_v1;
typedef struct muxiva_frame_view_v1 { muxiva_frame_header_v1 header; muxiva_frame_payload_v1 payload; } muxiva_frame_view_v1;

typedef struct muxiva_abort_reason_v1 {
  uint32_t abi_version; uint32_t struct_size; int32_t category; int32_t stage;
  muxiva_str_v1 code; muxiva_str_v1 message;
} muxiva_abort_reason_v1;

typedef muxiva_status_v1 (*muxiva_node_simple_fn_v1)(void *, muxiva_error_v1 *);
typedef muxiva_status_v1 (*muxiva_node_process_fn_v1)(void *, const muxiva_frame_view_v1 *,
                                                  muxiva_frame_view_v1 *, muxiva_error_v1 *);
typedef muxiva_status_v1 (*muxiva_node_signal_fn_v1)(void *, const muxiva_frame_view_v1 *, muxiva_error_v1 *);
typedef void (*muxiva_node_abort_fn_v1)(void *, const muxiva_abort_reason_v1 *) ;
typedef void (*muxiva_node_destroy_fn_v1)(void *);

typedef struct muxiva_node_vtable_v1 {
  uint32_t abi_version; uint32_t struct_size; void *user_data;
  muxiva_node_simple_fn_v1 on_prepare;
  muxiva_node_process_fn_v1 on_process;
  muxiva_node_signal_fn_v1 on_signal;
  muxiva_node_simple_fn_v1 on_finish;
  muxiva_node_abort_fn_v1 on_abort;
  muxiva_node_destroy_fn_v1 destroy;
  uint64_t capabilities;
  uint64_t reserved[3];
} muxiva_node_vtable_v1;

typedef muxiva_status_v1 (*muxiva_node_factory_create_fn_v1)(
    void *, muxiva_str_v1, muxiva_node_vtable_v1 *, muxiva_error_v1 *);
typedef struct muxiva_node_factory_v1 {
  uint32_t abi_version; uint32_t struct_size;
  muxiva_str_v1 node_type; muxiva_str_v1 version;
  muxiva_str_v1 input_port; muxiva_str_v1 output_port;
  void *user_data; muxiva_node_factory_create_fn_v1 create;
  uint64_t reserved[4];
} muxiva_node_factory_v1;

typedef struct muxiva_named_frame_v1 {
  muxiva_str_v1 output_port;
  muxiva_frame_view_v1 frame;
} muxiva_named_frame_v1;
enum { MUXIVA_NODE_METRIC_COUNTER_ADD = 1, MUXIVA_NODE_METRIC_GAUGE_SET = 2 };
typedef struct muxiva_node_metric_v1 {
  muxiva_str_v1 name;
  uint32_t operation;
  uint32_t reserved0;
  uint64_t value;
} muxiva_node_metric_v1;
typedef muxiva_status_v1 (*muxiva_graph_node_process_fn_v1)(
    void *, const muxiva_frame_view_v1 *, muxiva_str_v1,
    const muxiva_named_frame_v1 **, size_t *, muxiva_error_v1 *);
typedef struct muxiva_graph_node_vtable_v1 {
  uint32_t abi_version; uint32_t struct_size; void *user_data;
  muxiva_node_simple_fn_v1 on_prepare;
  muxiva_graph_node_process_fn_v1 on_process;
  muxiva_node_signal_fn_v1 on_signal;
  muxiva_node_simple_fn_v1 on_finish;
  muxiva_node_abort_fn_v1 on_abort;
  muxiva_node_destroy_fn_v1 destroy;
  uint64_t capabilities;
  uint64_t reserved[3];
  uint64_t (*take_next_source_tick_ns)(void *);
  void (*take_metrics)(void *, const muxiva_node_metric_v1 **, size_t *);
} muxiva_graph_node_vtable_v1;
typedef muxiva_status_v1 (*muxiva_multimodal_node_factory_create_fn_v1)(
    void *, muxiva_str_v1, muxiva_str_v1, muxiva_graph_node_vtable_v1 *, muxiva_error_v1 *);
typedef struct muxiva_multimodal_node_factory_v1 {
  uint32_t abi_version; uint32_t struct_size;
  muxiva_str_v1 node_type; muxiva_str_v1 version;
  uint32_t kind; uint32_t reserved0;
  muxiva_str_v1 ports_json; muxiva_str_v1 config_schema_json;
  void *user_data; muxiva_multimodal_node_factory_create_fn_v1 create;
  uint64_t reserved[4];
} muxiva_multimodal_node_factory_v1;
typedef struct muxiva_graph_run_summary_v1 {
  uint32_t abi_version; uint32_t struct_size; uint32_t worker_total;
  uint64_t reserved[4];
} muxiva_graph_run_summary_v1;

uint32_t muxiva_abi_version_v1(void);
uint64_t muxiva_capabilities_v1(void);
muxiva_status_v1 muxiva_runtime_create_v1(muxiva_runtime_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_runtime_release_v1(muxiva_runtime_v1);
muxiva_status_v1 muxiva_session_create_v1(muxiva_runtime_v1, muxiva_session_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_session_release_v1(muxiva_session_v1);
typedef struct muxiva_ingress_config_v1 { uint32_t abi_version; uint32_t struct_size; size_t item_capacity; size_t byte_capacity; } muxiva_ingress_config_v1;
typedef struct muxiva_ingress_stats_v1 { uint32_t abi_version; uint32_t struct_size; uint64_t accepted; uint64_t full_drops; uint64_t closed_drops; size_t queued_items; size_t queued_bytes; } muxiva_ingress_stats_v1;
muxiva_status_v1 muxiva_session_ingress_create_v1(muxiva_session_v1, const muxiva_ingress_config_v1 *, muxiva_session_ingress_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_session_ingress_clone_v1(muxiva_session_ingress_v1, muxiva_session_ingress_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_session_ingress_close_v1(muxiva_session_ingress_v1);
muxiva_status_v1 muxiva_session_ingress_release_v1(muxiva_session_ingress_v1);
muxiva_status_v1 muxiva_session_ingress_try_submit_v1(muxiva_session_ingress_v1, const muxiva_frame_view_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_session_ingress_stats_v1(muxiva_session_ingress_v1, muxiva_ingress_stats_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_session_ingress_try_pop_v1(muxiva_session_ingress_v1, muxiva_frame_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_frame_copy_v1(const muxiva_frame_view_v1 *, muxiva_frame_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_frame_release_v1(muxiva_frame_v1);
muxiva_status_v1 muxiva_frame_retain_v1(muxiva_frame_v1); /* always UNSUPPORTED in v1 */
muxiva_status_v1 muxiva_node_create_v1(const muxiva_node_vtable_v1 *, muxiva_node_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_node_release_v1(muxiva_node_v1);

/* Focused Stage-7 graph harness: source -> foreign transform -> Rust sink. */
muxiva_status_v1 muxiva_runtime_run_text_v1(muxiva_runtime_v1, muxiva_node_v1,
                                         const muxiva_frame_view_v1 *,
                                         char *output, size_t output_capacity,
                                         size_t *output_len, muxiva_error_v1 *);
muxiva_status_v1 muxiva_runtime_run_graph_v1(
    muxiva_runtime_v1, muxiva_str_v1,
    const muxiva_node_factory_v1 *, size_t, uint64_t,
    muxiva_graph_run_summary_v1 *, muxiva_error_v1 *);
muxiva_status_v1 muxiva_runtime_run_multimodal_graph_v1(
    muxiva_runtime_v1, muxiva_str_v1,
    const muxiva_multimodal_node_factory_v1 *, size_t, uint64_t,
    muxiva_graph_run_summary_v1 *, muxiva_error_v1 *);

#ifdef __cplusplus
}
#endif

#if defined(__cplusplus)
static_assert(sizeof(muxiva_token_v1) == 16, "muxiva token ABI drift");
#else
_Static_assert(sizeof(muxiva_token_v1) == 16, "muxiva token ABI drift");
#endif

#endif

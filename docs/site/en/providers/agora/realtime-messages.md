# Agora Realtime Messages

`agora.data_source` and `agora.data_sink` carry client commands and Agent events over the same
shared RTC session as audio. They are C++ Nodes, not a new Core abstraction.

| Node | Port | Schema |
| --- | --- | --- |
| `agora.data_source` | `message_out` Byte output | `application/vnd.muxiva.client-command+json`, max 1 KiB |
| `agora.data_sink` | `message_in` Byte input | `application/vnd.muxiva.client-event+json`, max 1 KiB |

The data stream is reliable and ordered. Muxiva paces outbound messages below Agora's 6 KiB/s
limit. `builtin.client_event_encoder` serializes `EventFrame` values as
`muxiva.client-event/v1`; messages larger than one packet use `muxiva.transport-fragment/v1` and are
reassembled by the client.

All Agora Nodes acquire one process-wide shared session with the configured Bot UID. In the
first isolation model, only the configured Browser UID is accepted. Audio and data from other
participants are ignored.

NotificationBus is not involved in this path. It remains local observability only.

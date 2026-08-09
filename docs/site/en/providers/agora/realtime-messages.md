# Agora Realtime Messages

`agora.data_source` and `agora.data_sink` carry client commands and Agent events over the same
shared RTC session as audio. They are C++ Nodes, not a new Core abstraction.

| Node | Port | Schema |
| --- | --- | --- |
| `agora.data_source` | `message_out` Byte output | `application/vnd.muxiva.client-command+json`, max 1 KiB |
| `agora.data_sink` | `message_in` Byte input | Application bytes, max 32 KiB; transport packets max 1 KiB |

The data stream is reliable and ordered. Muxiva paces outbound packets below Agora's 6 KiB/s
limit. The Voice Agent example's project-local `voice_room.event_encoder` maps `EventFrame` values
to its `muxiva.client-event/v1` application contract. `agora.data_sink` owns the transport concern:
messages larger than one packet use `muxiva.transport-fragment/v1` and are reassembled by the client.

This split is intentional. Client schemas and voice cancellation policy belong to an application
Node, while RTC packet limits belong to the Agora transport Node. Neither is a Runtime Core builtin.

All Agora Nodes acquire one process-wide shared session with the configured Bot UID. In the
first isolation model, only the configured Browser UID is accepted. Audio and data from other
participants are ignored.

NotificationBus is not involved in this path. It remains local observability only.

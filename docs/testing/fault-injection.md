# Fault-injection matrix

| Boundary | Deterministic fault | Required invariant |
| --- | --- | --- |
| Frame | malformed layout, metadata, lineage | rejected before payload access |
| Graph/Node | lifecycle error or panic | one abort and reverse cleanup |
| Edge/Queue | full, closed, stalled consumer | bounded and policy-visible outcome |
| Managed stream | timeout, reconnect, late result | admission released; late output discarded |
| Signal/EventBus | slow or failing subscriber | publisher and other subscribers remain isolated |
| C/C++ ABI | short struct, stale handle, exception | no unwind; stable error; owned copy |
| Mock RTC | loss, reorder, disconnect, late callback | callback nonblocking; context lives through drain |
| Foreign domain | full inbox, deadline, late completion | per-domain isolation and exactly one abort |
| Studio | invalid graph, forged token, occupied port | same validator, authorization, explicit failure |

Every new adapter registers a scenario for malformed input, shutdown racing
with work, and ownership release before it can be described as supported.


# D13: Connection-owned Sessions

## Decision

Every accepted client transport connection owns exactly one Muxiva Session:

```text
accepted connection
  └─ Session
      ├─ one graph instance
      ├─ one Turn Controller
      ├─ one Agent instance and history
      ├─ bounded ingress/egress queues
      └─ one transport binding
```

There is no Session Router in the media or command data path. An acceptor only
authenticates a new connection, creates a Session from a configured graph
factory, binds that connection to the Session transport ports, and transfers
ownership. After that hand-off, audio, text, events, endpoint commands, ACKs,
cancellation, and close all remain inside that Session.

## Framework boundary

Muxiva Core owns only generic lifecycle and flow-control concepts:

- `SessionFactory`: creates an isolated graph instance from immutable
  deployment configuration;
- `Session`: owns all mutable per-client state and closes it as one unit;
- `SessionSupervisor`: tracks lifecycle, quotas, observability, and cleanup but
  never forwards application Frames between Sessions;
- typed Frames, Events, Signals, bounded queues, cancellation, and deadlines;
- a transport binding whose ingress and egress belong to one Session.

Core does not know ESP32, Xiaozhi, `show_image`, speaker volume, weather,
artwork, or any Agent implementation. Provider clients and immutable model
metadata may be shared as infrastructure; conversation state, playback clocks,
queues, cancellation generations, and endpoint command correlation maps may
not be shared.

## Endpoint commands

An Agent may emit a generic command-request Event on its existing `event_out`.
The graph decides whether that Event reaches a transport adapter. The adapter
uses deployment configuration for:

- accepted Event topics;
- allowed command types;
- protocol envelope mapping;
- timeouts and ACK policy.

Command names and endpoint implementations belong to deployment/provider
configuration and endpoint firmware, not Core. Agents never discover endpoint
IP addresses or open reverse HTTP/WebSocket connections. A command is reported
as *applied* only after a correlated ACK; queue acceptance may only be reported
as *sent* or *queued*.

## Lifecycle and isolation

1. Acceptor authenticates connection `C`.
2. `SessionFactory.create(C.metadata)` creates Session `S` and a fresh graph.
3. `C` is moved into `S.transport`; no global current socket exists.
4. Session-scoped Tasks process ingress and egress with bounded queues.
5. Disconnect, timeout, or fatal graph error cancels `S`, closes its Agent and
   provider sessions, drains/drops its own queues according to policy, and
   releases resources.
6. A reconnect creates a new Session unless an explicit, authenticated resume
   contract restores a bounded snapshot. It never attaches to another active
   Session by mutable global ID.

One Session failure cannot clear, pause, or overwrite another Session's
transport or Agent state.

## Required conformance tests

- two simultaneous connections create two graph and Agent instances;
- audio and commands from A are never observable on B;
- interrupting A does not cancel B;
- disconnecting A releases all A Tasks and bounded queues;
- a slow A egress cannot add latency or backpressure to B;
- endpoint commands are disabled unless explicitly configured;
- disallowed command types never leave the Session;
- command IDs correlate ACKs to the originating Session and turn;
- reconnect cannot receive queued output from the previous connection.

## Migration of the Xiaozhi prototype

The current Python Xiaozhi gateway is a single-device prototype: it owns a
mutable current WebSocket and connects graph Node processes through a loopback
control socket. It is acceptable only for one-device development and is not the
multi-user serving contract.

Migration removes the singleton gateway and loopback control server. The
Xiaozhi acceptor will create one graph Session per accepted WebSocket and bind
the provider source/sinks directly to that Session-owned transport object. The
external device still opens exactly one WebSocket, and no reverse device HTTP
control path remains.

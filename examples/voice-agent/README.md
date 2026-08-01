# Voxa Voice Agent application

This application demonstrates the provider boundary used by Voxa:

- Qwen intelligence is implemented only by application-owned Python Node Packs
  under `.voxa/nodes/qwen_*`;
- Agora transport is implemented only in C++ under `providers/agora/cpp` and
  `.voxa/nodes/agora_*`;
- Voxa Core, Graph builtins, and Studio know only typed Frames, Node Pack
  Manifests, and generic Connection fields.

Open this directory's `graph.json` with Studio. Studio discovers the project's
Node Packs and the two graph templates under `.voxa/templates`; it never embeds
Qwen or Agora registration code. Secrets entered in **Connections** stay in
process memory and are forwarded only through environment names declared by the
owning Node Pack Manifest.

## Current executable boundary

The Python Qwen Realtime Node Pack is executable through Studio's generic
Python Host and has credential-free protocol tests. The Agora Node Packs compile
against Voxa's C++ ABI, but Studio's generic compiled-C++ Node Pack loader is not
implemented yet. Therefore the live templates are intentionally shown as
unavailable instead of silently substituting a mock. The cascade template also
remains unavailable until its ASR, LLM, TTS, VAD, and turn-context Nodes land.

Run the offline gates from the repository root:

```sh
./scripts/check-provider-boundaries.sh
./scripts/check-voice-node-packs.sh
```

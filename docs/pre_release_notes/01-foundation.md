# Muxiva Pre-release Notes: Stage 1 Foundation

Date: **2026-07-31**

Contract version: **0.1.0-draft.1**

## Summary

Stage 1 establishes Muxiva's product scope, terminology, system boundaries, and
technical contract. It intentionally contains no runtime implementation.

The first v0.1 validation target is a mock real-time voice-agent graph, while
the Rust Core remains a general multimodal runtime for Audio, Video, Text, and
Byte frames.

## Added

- Project README with v0.1 scope, principles, status, and planned layout.
- Normative product and technical contract covering:
  - Node, Stream, Graph, Frame, Port, Edge, Adapter, and Stage terminology;
  - GraphBuilder, JSON GraphDefinition, Node Registry, CLI, and Studio roles;
  - Rust Core and C++, Python, TypeScript, and Adapter boundaries;
  - Frame header, timestamp, error, logging, and metrics minima;
  - immutable ownership and versioned C ABI rules;
  - callback-thread restrictions and deterministic stop ordering;
  - lifecycle and exactly-once abort behavior;
  - Semantic Versioning and compatibility policy; and
  - gated inputs, outputs, and acceptance criteria for Stages 2 through 6.

## Decisions

1. Muxiva validates a real-time voice-agent vertical first, using mock ASR, LLM,
   TTS, transport, and sink nodes.
2. ASR, LLM, TTS, conversation state, and prompts are Node concerns and do not
   become special Rust Core APIs.
3. `GraphDefinition` is the only graph protocol shared by programmatic, JSON,
   CLI, Runtime, and web Studio surfaces.
4. `Frame` is the only node-to-node information family. Signal and Event are
   explicit Frame variants with constrained routing.
5. Copy is the default native-buffer ownership mode. Retain/Release is an
   opt-in capability requiring an explicit thread-safe SDK guarantee.
6. Runtime behavior is implemented only after the corresponding stage gate is
   accepted.

## Non-goals for this stage

- Cargo workspace or Rust source code
- Runtime graph execution
- Frame implementations
- CLI or web Studio implementation
- C++, Python, or TypeScript bindings
- RTC, FFmpeg, or model-service integrations
- CI and executable test infrastructure

## Verification

Stage 1 is verified by checking the three required files, scanning for
unfinished placeholders, checking terminology and cross-document consistency,
and running `git diff --check`. Executable unit tests begin with Stage 2.

## Known risks

- The initial JSON schema may expose compatibility questions when concrete
  Rust types are introduced. Stage 4 must preserve the semantic contract and
  explicitly version any necessary representation change.
- Cross-language performance is intentionally not optimized before ownership
  correctness is tested.
- Real SDK callback and release-thread behavior remains unvalidated until a
  later Adapter stage; the contract therefore defaults to copying.
- Open-source license, governance, security policy, and release signing are not
  selected yet and must be resolved before the first public release.

## Next stage recommendation

After Stage 1 approval, begin Stage 2 only: create the Rust Edition 2021 Cargo
workspace, foundation crates and types, replaceable logging, hello example,
minimal CI, and focused tests. Do not introduce Tokio, frames, bindings, media
SDKs, or runtime concurrency in Stage 2.

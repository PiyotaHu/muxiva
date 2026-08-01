# Providers

Providers connect Voxa to RTC SDKs, media libraries, model APIs, transports, and
devices. They remain outside Runtime Core and must preserve bounded ingress,
ownership, cancellation, and shutdown contracts.

## Agora

The repository contains:

- an in-memory Mock RTC contract;
- an optional Agora C++ adapter for PCM16 audio and I420 video;
- an optional Python audio provider;
- scripts for credentialed live-room and soak testing.

Live credentials are never committed. Production certification still requires
retained per-platform live-room evidence, long-duration tests, reconnect and
late-callback faults, and release-specific SDK compatibility.

## FFmpeg

The optional media layer provides streaming audio resampling and video scale or
color conversion, including RGBA8 and I420 paths. FFmpeg remains an optional
provider dependency rather than a Core requirement.

## Provider acceptance checklist

A provider proposal must define:

- supported SDK versions, platforms, architectures, and licenses;
- input/output Frame schemas and clock behavior;
- callback threads and ownership transfer;
- queue and byte bounds, backpressure, and overflow behavior;
- cancellation, reconnect, late callback, and shutdown behavior;
- mock, fault-injection, live, and soak test strategy;
- metrics, diagnostics, credentials, and secret handling.

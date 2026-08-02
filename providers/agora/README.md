# Agora Provider

The Agora provider is implemented entirely in C++ under `cpp`. The directory
contains the RTC adapter, native SDK boundary, source and sink Node Packs,
manifests, build definition, and tests.

An offline stub build is available for CI. A real build requires the Agora
Native SDK and `-DVOXA_ENABLE_AGORA=ON`; compiled Node Pack artifacts are placed
under the configured `VOXA_NODE_PACK_OUTPUT_ROOT`.

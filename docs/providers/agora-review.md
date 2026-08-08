# Agora adapter review record

| Item | D07 decision |
| --- | --- |
| SDK | Agora RTC Native 4.x C++ header contract only |
| License/distribution | Optional vendor dependency; not vendored or redistributed by Muxiva |
| Dynamic libraries | Supplied and packaged by the application for its target platform |
| Buffer ownership | Muxiva copies PCM16/I420 before every callback returns |
| Threading | Vendor callbacks are admission-only; SDK control is serialized on one C++ thread |
| Backpressure | Fixed-capacity external ingress/queue; full means observable drop |
| Shutdown | Stop admission, unregister, drain, destroy tracks, release engine, close ingress |
| Late callbacks | Observer is detached atomically; test injects a callback after SDK shutdown |
| Credentials | Environment only; no secret persistence or log output |
| Offline evidence | C++ fake-SDK contract + ASan/UBSan and native header compile |
| Remaining gate | Credentialed live-room soak and target-platform vendor-binary certification |

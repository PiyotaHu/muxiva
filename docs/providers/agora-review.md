# Agora adapter review record

| Item | D07 decision |
| --- | --- |
| SDK | Agora RTC Native 4.x header contract; Python community SDK 3.4.2.1 |
| License/distribution | Optional vendor/community dependencies; not vendored or redistributed by Voxa |
| Dynamic libraries | Supplied and packaged by the application for its target platform |
| Buffer ownership | Voxa copies PCM16/I420 before every callback returns |
| Threading | Vendor callbacks are admission-only; SDK control is serialized on one C++ thread |
| Backpressure | Fixed-capacity external ingress/queue; full means observable drop |
| Shutdown | Stop admission, unregister, drain, destroy tracks, release engine, close ingress |
| Late callbacks | Observer is detached atomically; test injects a callback after SDK shutdown |
| Credentials | Environment only; no secret persistence or log output |
| Offline evidence | C++ fake-SDK contract + ASan/UBSan, native header compile, real Python import probe |
| Remaining gate | Credentialed live-room soak and target-platform vendor-binary certification |


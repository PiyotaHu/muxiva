# RTC adapter review template

Before adding an RTC SDK, record: SDK/version and licenses; dynamic libraries;
callback threads and ordering; connect/leave/destroy guarantees; media buffer
owner, lifetime, retain/release operations and required release thread;
late-callback guarantee; error/exception behavior; reconnect/recording/user
events; deployment architecture; and the Mock RTC contract-test scenario used.
No SDK becomes a Core dependency until this review and the shared callback
drain/late-callback tests pass.

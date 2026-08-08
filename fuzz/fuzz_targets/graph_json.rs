#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        if let Ok(document) = muxiva_graph_json::parse(input) {
            let _ = muxiva_graph_json::compile(&document);
        }
    }
});

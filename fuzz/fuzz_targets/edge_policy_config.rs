#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let overflow = String::from_utf8_lossy(data);
    let input = serde_json_text(&overflow);
    if let Ok(document) = muxiva_graph_json::parse(&input) {
        let _ = muxiva_graph_json::compile(&document);
    }
});

fn serde_json_text(overflow: &str) -> String {
    let escaped = overflow
        .chars()
        .flat_map(|character| character.escape_default())
        .collect::<String>();
    format!(
        r#"{{"version":"muxiva.graph/v1","graph_id":"fuzz","nodes":[],"edges":[{{"id":"e","from":{{"node_id":"a","port":"out"}},"to":{{"node_id":"b","port":"in"}},"frame_type":"text","queue_policy":{{"capacity":1,"overflow":"{escaped}"}}}}]}}"#
    )
}

use voxa_graph_json::{compile, parse, GRAPH_V1_SCHEMA};

const VALID: &str = include_str!("../../../examples/graphs/text-uppercase.v1.json");

#[test]
fn graph_v1_compiles_through_graph_builder_with_explicit_identity() {
    let graph = compile(&parse(VALID).unwrap()).unwrap();
    assert_eq!(graph.graph_id().as_str(), "text-uppercase");
    assert_eq!(graph.nodes().len(), 3);
    assert_eq!(graph.edges().len(), 2);
}

#[test]
fn unknown_registration_and_zero_queue_report_field_diagnostics() {
    let invalid = VALID
        .replace("builtin.uppercase", "untrusted.shell")
        .replace("\"capacity\":32", "\"capacity\":0");
    let errors = compile(&parse(&invalid).unwrap()).unwrap_err();
    assert!(errors.iter().any(|error| error.pointer == "/nodes/1"));
    assert!(errors
        .iter()
        .any(|error| error.pointer.starts_with("/edges/")));
}

#[test]
fn schema_is_machine_readable_and_declares_v1() {
    let value: serde_json::Value = serde_json::from_str(GRAPH_V1_SCHEMA).unwrap();
    assert_eq!(value["properties"]["version"]["const"], "voxa.graph/v1");
}

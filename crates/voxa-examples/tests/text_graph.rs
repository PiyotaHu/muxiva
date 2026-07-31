use std::process::Command;

#[test]
fn text_graph_prints_deterministic_collected_output() {
    let output = Command::new(env!("CARGO_BIN_EXE_text_graph"))
        .output()
        .expect("the text_graph example must run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("example output is UTF-8"),
        "Collected uppercase text: HELLO, VOXA\n"
    );
}

use std::process::Command;

#[test]
fn control_plane_example_runs() {
    let output = Command::new(env!("CARGO_BIN_EXE_control_plane"))
        .output()
        .expect("run control-plane example");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "voxa.transport.turn.interrupted interrupted=true\n"
    );
}

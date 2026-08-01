use std::process::Command;

#[test]
fn text_graph_explains_its_dsl_lifecycle_and_result_with_voxa_branding() {
    let output = Command::new(env!("CARGO_BIN_EXE_text_graph"))
        .output()
        .expect("the text_graph example must run");

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("example output is UTF-8"),
        concat!(
            "[VOXA][INFO][graph.loaded] id=example.text-uppercase nodes=3 edges=2\n",
            "[VOXA][GRAPH] human-readable DSL\n",
            "graph \"example.text-uppercase\" {\n",
            "  node \"collect-sink\" kind=sink type=\"example.collect_sink\"\n",
            "    input text_in: text\n",
            "  node \"text-source\" kind=source type=\"example.text_source\"\n",
            "    output text_out: text\n",
            "  node \"uppercase-transform\" kind=transform type=\"example.uppercase_transform\"\n",
            "    input text_in: text\n",
            "    output text_out: text\n",
            "  edge \"source-to-uppercase\" text-source.text_out -> uppercase-transform.text_in frame=text queue=1/block\n",
            "  edge \"uppercase-to-sink\" uppercase-transform.text_out -> collect-sink.text_in frame=text queue=1/block\n",
            "}\n",
            "flow:\n",
            "  text-source\n",
            "    └─text-source.text_out [text] -> uppercase-transform.text_in\n",
            "  uppercase-transform\n",
            "    └─uppercase-transform.text_out [text] -> collect-sink.text_in\n",
            "[VOXA][INFO][runtime.started] mode=sync source_frames=2\n",
            "[VOXA][INFO][runtime.completed] status=success outputs=2\n",
            "[VOXA][RESULT] HELLO, VOXA\n",
        )
    );
}

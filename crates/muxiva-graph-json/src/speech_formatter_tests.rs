use super::{
    demo_frame, drain_presentable_text, ConfigMap, EventData, FramePayload, NamespacedName, Node,
    NodeContext, NodeId, PortName, SchemaVersion, SpeechFormatter, TextData, Value,
};

fn drain_fragments<'a>(
    fragments: impl IntoIterator<Item = &'a str>,
    minimum: usize,
    maximum: usize,
) -> Vec<String> {
    let mut buffer = String::new();
    let mut input = String::new();
    let mut chunks = Vec::new();
    for fragment in fragments {
        input.push_str(fragment);
        buffer.push_str(fragment);
        drain_presentable_text(&mut buffer, minimum, maximum, false, &mut chunks);
        assert_eq!(
            format!("{}{buffer}", chunks.concat()),
            input,
            "streaming byte preservation"
        );
        assert!(
            buffer.chars().count() <= maximum * 2,
            "unbounded pending text"
        );
        assert_bounded(&chunks, maximum);
    }
    drain_presentable_text(&mut buffer, minimum, maximum, true, &mut chunks);
    assert!(buffer.is_empty());
    assert_bounded(&chunks, maximum);
    assert_eq!(chunks.concat(), input, "completion byte preservation");
    chunks
}
fn assert_bounded(chunks: &[String], maximum: usize) {
    for chunk in chunks {
        assert!(!chunk.is_empty());
        assert!(
            chunk.chars().count() <= maximum,
            "exceeds {maximum}: {chunk:?}"
        );
    }
}
fn intact(chunks: &[String], input: &str, tokens: &[&str]) {
    let mut boundary = 0;
    for chunk in chunks {
        boundary += chunk.len();
        for token in tokens {
            assert!(input.contains(token));
            for (start, _) in input.match_indices(token) {
                assert!(
                    boundary <= start || boundary >= start + token.len(),
                    "byte {boundary} splits {token:?}: {chunks:?}"
                );
            }
        }
    }
}
#[test]
fn speech_formatter_decimal_temperature_and_percentage_are_not_sentences() {
    let text = "Temperature is 26.25°C and humidity is 68.75%.";
    assert_eq!(drain_fragments([text], 1, 80), [text]);
}
#[test]
fn speech_formatter_waits_for_the_digit_after_a_streamed_decimal_point() {
    let mut buffer = String::from("当前温度为26.");
    let mut chunks = Vec::new();
    drain_presentable_text(&mut buffer, 1, 80, false, &mut chunks);
    assert!(chunks.is_empty());
    assert_eq!(buffer, "当前温度为26.");
    buffer.push_str("25°C，湿度为68.");
    drain_presentable_text(&mut buffer, 1, 80, false, &mut chunks);
    assert!(chunks.is_empty());
    buffer.push_str("75%，体感舒适。");
    drain_presentable_text(&mut buffer, 1, 80, false, &mut chunks);
    assert_eq!(chunks, ["当前温度为26.25°C，湿度为68.75%，体感舒适。"]);
    assert!(buffer.is_empty());
}
#[test]
fn speech_formatter_numeric_sentence_end_is_confirmed_by_following_text() {
    let mut buffer = String::from("The answer is 26.");
    let mut chunks = Vec::new();
    drain_presentable_text(&mut buffer, 1, 80, false, &mut chunks);
    assert!(chunks.is_empty());
    buffer.push_str(" Next question.");
    drain_presentable_text(&mut buffer, 1, 80, false, &mut chunks);
    assert_eq!(
        chunks.first().map(String::as_str),
        Some("The answer is 26.")
    );
    drain_presentable_text(&mut buffer, 1, 80, true, &mut chunks);
    assert_eq!(chunks, ["The answer is 26.", " Next question."]);
}
#[test]
fn speech_formatter_flush_resolves_an_ambiguous_numeric_sentence_end() {
    let mut buffer = String::from("The answer is 26.");
    let mut chunks = Vec::new();
    drain_presentable_text(&mut buffer, 1, 80, false, &mut chunks);
    assert!(chunks.is_empty());
    drain_presentable_text(&mut buffer, 1, 80, true, &mut chunks);
    assert_eq!(chunks, ["The answer is 26."]);
}
#[test]
fn speech_formatter_prefers_a_complete_sentence_to_an_earlier_comma() {
    let text = "First clause, the sentence ends here. Next.";
    assert_eq!(
        drain_fragments([text], 5, 40),
        ["First clause, the sentence ends here.", " Next."]
    );
}
#[test]
fn speech_formatter_long_sentence_prefers_a_comma_to_a_later_space() {
    let text = "alpha beta, gamma delta epsilon zeta";
    let chunks = drain_fragments([text], 5, 24);
    assert_eq!(chunks.first().map(String::as_str), Some("alpha beta,"));
    intact(
        &chunks,
        text,
        &["alpha", "beta", "gamma", "delta", "epsilon", "zeta"],
    );
}
#[test]
fn speech_formatter_long_sentence_uses_a_semicolon_boundary() {
    let text = "第一部分已完成；第二部分仍在等待后续输入并继续检查直到完成";
    let chunks = drain_fragments([text], 5, 20);
    assert_eq!(chunks.first().map(String::as_str), Some("第一部分已完成；"));
}
#[test]
fn speech_formatter_long_english_text_keeps_short_words_intact() {
    let text = "We checked temperature and humidity measurements before reporting the result";
    let chunks = drain_fragments([text], 5, 20);
    intact(&chunks, text, &text.split_whitespace().collect::<Vec<_>>());
}
#[test]
fn speech_formatter_long_mixed_text_keeps_numeric_units_intact() {
    let text = "最新测量结果显示当前温度为-12.75°C，湿度为68.75%，随后温度将升至+26.25°C。";
    let chunks = drain_fragments([text], 5, 20);
    intact(&chunks, text, &["-12.75°C", "68.75%", "+26.25°C"]);
}
#[test]
fn speech_formatter_decimal_near_maximum_moves_with_its_whole_token() {
    let fragments = ["Measured value is 26.", "25°C and stable."];
    let chunks = drain_fragments(fragments, 5, 24);
    intact(
        &chunks,
        &fragments.concat(),
        &["Measured", "value", "26.25°C", "stable"],
    );
}
#[test]
fn speech_formatter_punctuation_runs_stay_with_the_preceding_sentence() {
    let text = "完成！？ Really?! 继续。";
    assert_eq!(
        drain_fragments([text], 1, 80),
        ["完成！？", " Really?!", " 继续。"]
    );
}
#[test]
fn speech_formatter_a_sentence_beyond_maximum_does_not_bypass_the_limit() {
    let text = format!("{}。", "甲".repeat(65));
    assert!(drain_fragments([text.as_str()], 5, 20).len() >= 4);
}
#[test]
fn speech_formatter_minimum_can_combine_short_sentences() {
    let text = "Hi. 很好。This sentence is long enough.";
    assert_eq!(drain_fragments([text], 12, 60), [text]);
}
#[test]
fn speech_formatter_flush_emits_an_unpunctuated_tail() {
    let mut buffer = String::from("没有结尾标点的最后一段");
    let mut chunks = Vec::new();
    drain_presentable_text(&mut buffer, 20, 80, false, &mut chunks);
    assert!(chunks.is_empty());
    drain_presentable_text(&mut buffer, 20, 80, true, &mut chunks);
    assert_eq!(chunks, ["没有结尾标点的最后一段"]);
    drain_presentable_text(&mut buffer, 20, 80, true, &mut chunks);
    assert_eq!(chunks.len(), 1);
}
#[test]
fn speech_formatter_empty_or_whitespace_completion_emits_nothing() {
    for text in ["", " \t\r\n"] {
        let mut buffer = text.to_owned();
        let mut chunks = Vec::new();
        drain_presentable_text(&mut buffer, 20, 80, true, &mut chunks);
        assert!(chunks.is_empty());
        assert!(buffer.is_empty());
    }
}
#[test]
fn speech_formatter_oversized_tokens_have_a_bounded_lossless_fallback() {
    for text in ["a".repeat(257), "7".repeat(257), "！".repeat(257)] {
        let fragments = text
            .char_indices()
            .map(|(index, character)| &text[index..index + character.len_utf8()]);
        assert!(drain_fragments(fragments, 5, 20).len() >= 13);
    }
}
#[test]
fn speech_formatter_utf8_and_emoji_survive_fallback_boundaries() {
    let text = "温度🙂正常🚀继续记录👩‍💻測定値を確認して温度🙂正常🚀继续记录👩‍💻最终完成";
    assert!(drain_fragments([text], 5, 20).len() > 1);
}
#[test]
fn speech_formatter_every_fragment_size_preserves_mixed_content_and_tokens() {
    let text =
        "当前温度为26.25°C，humidity is 68.75%; status remains comfortable. 下一次温度为-12.5°C！";
    let mut offsets = text
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    offsets.push(text.len());
    let count = offsets.len() - 1;
    for size in 1..=count {
        let fragments = (0..count)
            .step_by(size)
            .map(|start| &text[offsets[start]..offsets[(start + size).min(count)]]);
        let chunks = drain_fragments(fragments, 5, 24);
        intact(
            &chunks,
            text,
            &[
                "26.25°C",
                "humidity",
                "68.75%",
                "status",
                "remains",
                "comfortable",
                "-12.5°C",
            ],
        );
    }
}
#[test]
fn speech_formatter_token_boundary_can_fall_below_minimum() {
    let text = "甲abcdefghijklmnopqrst乙";
    let chunks = drain_fragments([text], 20, 20);
    assert_eq!(chunks.first().map(String::as_str), Some("甲"));
    intact(&chunks, text, &["abcdefghijklmnopqrst"]);
}
#[test]
fn speech_formatter_leading_fraction_and_grouped_number_are_preserved() {
    let text = "The fraction is .5 and time is 09:30, with 1,234.56 units.";
    let chunks = drain_fragments([text], 5, 24);
    intact(&chunks, text, &[".5", "09:30", "1,234.56"]);
}

#[test]
fn speech_formatter_punctuation_run_does_not_consume_a_leading_fraction() {
    let text = "完成！.5 is enough.";
    let chunks = drain_fragments([text], 1, 80);
    assert_eq!(chunks, ["完成！", ".5 is enough."]);
    intact(&chunks, text, &[".5"]);
}

#[test]
fn speech_formatter_punctuation_run_waits_for_a_streamed_fraction() {
    let fragments = ["完成！.", "5 is enough."];
    let chunks = drain_fragments(fragments, 1, 80);
    assert_eq!(chunks, ["完成！", ".5 is enough."]);
    intact(&chunks, &fragments.concat(), &[".5"]);
}

// Exercise the actual Node entry point and derived Text Frames, not just
// raw chunk boundaries: formatting after a split can change spoken meaning.
fn spoken_fragments<'a>(
    fragments: impl IntoIterator<Item = &'a str>,
    minimum: usize,
    maximum: usize,
) -> Vec<String> {
    let mut formatter = SpeechFormatter {
        code_block_message: "Code is available in chat.".into(),
        table_message: "Table is available in chat.".into(),
        strip_urls: true,
        maximum_chunk_characters: maximum,
        minimum_chunk_characters: minimum,
        in_fenced_code: false,
        in_table: false,
        pending_backticks: 0,
        in_bare_url: false,
        active_sequence: None,
        pending_text: String::new(),
        suppressed_parenthetical_terms: Vec::new(),
        pending_parenthetical: String::new(),
        parenthetical_closing: None,
    };
    let mut inputs = fragments
        .into_iter()
        .map(|fragment| ("text_in", FramePayload::Text(TextData::new(fragment))))
        .collect::<Vec<_>>();
    inputs.push((
        "event_in",
        FramePayload::Event(EventData::new(
            NamespacedName::new("muxiva.agent.response.completed").unwrap(),
            SchemaVersion::new(1).unwrap(),
            NodeId::new("agent-test").unwrap(),
            Value::Bool(true),
        )),
    ));
    let mut spoken = Vec::new();
    for (port, payload) in inputs {
        let mut context = NodeContext::new(
            NodeId::new("formatter-test").unwrap(),
            ConfigMap::empty(),
            Some(PortName::new(port).unwrap()),
        );
        let frame = demo_frame("formatter-test", 10, payload).unwrap();
        formatter.on_process(Some(frame), &mut context).unwrap();
        for emission in context.emissions() {
            assert_eq!(emission.output_port().as_str(), "text_out");
            assert_eq!(emission.frame().header().sequence_id().get(), 10);
            spoken.push(
                emission
                    .frame()
                    .as_text()
                    .unwrap()
                    .data()
                    .as_str()
                    .to_owned(),
            );
        }
    }
    spoken
}

#[test]
fn speech_formatter_node_emits_intact_streaming_decimal_values() {
    assert_eq!(
        spoken_fragments(["当前温度为26.", "25°C，湿度68.", "75%。"], 1, 80),
        ["当前温度为26.25°C，湿度68.75%。"],
    );
    assert_eq!(
        spoken_fragments(["完成！.", "5 is enough."], 1, 80),
        ["完成！", ".5 is enough."]
    );
}

#[test]
fn speech_formatter_node_keeps_url_suppression_across_forced_boundaries() {
    for text in [
        "Visit https://example.com",
        "Visit https://example.com/averylongpaththatmustspanseveralchunks and continue.",
    ] {
        let mut offsets = text
            .char_indices()
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        offsets.push(text.len());
        let count = offsets.len() - 1;
        for size in 1..=count {
            let fragments = (0..count)
                .step_by(size)
                .map(|start| &text[offsets[start]..offsets[(start + size).min(count)]]);
            let spoken = spoken_fragments(fragments, 5, 20).join(" ");
            assert_eq!(
                spoken,
                if text.ends_with("continue.") {
                    "Visit and continue."
                } else {
                    "Visit"
                },
                "URL must not leak into TTS input (fragment size {size})"
            );
        }
    }
}

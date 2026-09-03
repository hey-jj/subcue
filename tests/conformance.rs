mod support;

use std::collections::BTreeSet;

use serde_json::Value;
use subcue::{parse, Error, EventKind, Format, Subtitles, VttBlockKind};

fn document() -> Value {
    serde_json::from_str(include_str!("../conformance-vectors.json")).unwrap()
}

/// Records whose pinned coordinate is not the physical one.
///
/// Each entry names the record, the field, the value the shipped record
/// pins, and the physical value the parser reports. The record for SRT45
/// pins the byte before the invalid one: the 0xFF byte is at offset 36 and
/// offset 35 is the space before it. The three ASS records pin line 20 for
/// a Dialogue line that is physical line 18 of an 18-line file with LF
/// endings and no byte order mark. Line numbers are one-based physical
/// lines after byte order mark removal and offsets are physical byte
/// positions, so the parser reports the physical value. When a record is
/// corrected, its entry here fails and must be removed.
const PHYSICAL_COORDINATES: [(&str, &str, u64, usize); 4] = [
    ("SRT45", "offset", 35, 36),
    ("ASS40", "line", 20, 18),
    ("ASS44", "line", 20, 18),
    ("ASS45", "line", 20, 18),
];

fn physical_coordinate(record: &Value, field: &str, pinned: u64) -> usize {
    let id = record["id"].as_str().unwrap();
    match PHYSICAL_COORDINATES
        .iter()
        .find(|entry| entry.0 == id && entry.1 == field)
    {
        Some((_, _, stale, physical)) => {
            assert_eq!(pinned, *stale, "{id} was corrected; drop its entry");
            *physical
        }
        None => pinned as usize,
    }
}

fn format(value: &str) -> Format {
    match value {
        "srt" => Format::Srt,
        "vtt" => Format::Vtt,
        "ass" => Format::Ass,
        other => panic!("unknown format {other}"),
    }
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(text, 16).unwrap()
        })
        .collect()
}

fn bytes(record: &Value) -> Vec<u8> {
    if let Some(hex) = record["hex"].as_str() {
        return hex_bytes(hex);
    }
    match record["id"].as_str().unwrap() {
        "PERF01" => support::perf_srt(10_000),
        "PERF02" => support::perf_vtt(10_000),
        "PERF03" => support::perf_ass(10_000),
        id => panic!("missing bytes for {id}"),
    }
}

fn warning_set(subtitles: &Subtitles) -> BTreeSet<&'static str> {
    subtitles
        .warnings
        .iter()
        .map(support::warning_name)
        .collect()
}

fn expected_warning_set(expect: &Value) -> BTreeSet<&str> {
    expect["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|warning| warning.as_str().unwrap())
        .collect()
}

fn assert_cue_tuple(cue: &subcue::Cue, tuple: &Value) {
    let values = tuple.as_array().unwrap();
    assert_eq!(cue.start.0, values[0].as_i64().unwrap());
    assert_eq!(cue.end.0, values[1].as_i64().unwrap());
    assert_eq!(cue.text, values[2].as_str().unwrap());
}

fn assert_model(record: &Value, subtitles: &Subtitles) {
    let expect = &record["expect"];
    if let Some(count) = expect["cues"].as_u64() {
        assert_eq!(subtitles.cues.len(), count as usize, "{}", record["id"]);
    }
    if !expect["first"].is_null() {
        assert_cue_tuple(subtitles.cues.first().unwrap(), &expect["first"]);
    }
    if !expect["last"].is_null() {
        assert_cue_tuple(subtitles.cues.last().unwrap(), &expect["last"]);
    }
    if let Some(indices) = expect["indices"].as_array() {
        let actual: Vec<Option<u32>> = subtitles.cues.iter().map(|cue| cue.index).collect();
        let expected: Vec<Option<u32>> = indices
            .iter()
            .map(|value| value.as_u64().map(|number| number as u32))
            .collect();
        assert_eq!(actual, expected);
    }
    if let Some(ids) = expect["ids"].as_array() {
        let actual: Vec<Option<&str>> =
            subtitles.cues.iter().map(|cue| cue.id.as_deref()).collect();
        let expected: Vec<Option<&str>> = ids.iter().map(Value::as_str).collect();
        assert_eq!(actual, expected);
    }
    if let Some(settings) = expect["settings"].as_array() {
        let actual: Vec<Option<&str>> = subtitles
            .cues
            .iter()
            .map(|cue| cue.settings.as_deref())
            .collect();
        let expected: Vec<Option<&str>> = settings.iter().map(Value::as_str).collect();
        assert_eq!(actual, expected);
    }
    if let Some(order) = expect["order"].as_array() {
        let actual: Vec<i64> = subtitles.cues.iter().map(|cue| cue.start.0).collect();
        let expected: Vec<i64> = order.iter().map(|value| value.as_i64().unwrap()).collect();
        assert_eq!(actual, expected);
    }
    if let Some(styles) = expect["styles"].as_array() {
        let actual: Vec<&str> = subtitles
            .header
            .styles
            .iter()
            .map(|style| style.name.as_str())
            .collect();
        let expected: Vec<&str> = styles.iter().map(|value| value.as_str().unwrap()).collect();
        assert_eq!(actual, expected);
    }
    if let Some(events) = expect["events"].as_array() {
        let actual: Vec<&str> = subtitles
            .events
            .iter()
            .map(|event| match event.kind {
                EventKind::Comment => "Comment",
                EventKind::Sound => "Sound",
                EventKind::Picture => "Picture",
                EventKind::Movie => "Movie",
                EventKind::Command => "Command",
                EventKind::Dialogue => "Dialogue",
            })
            .collect();
        let expected: Vec<&str> = events.iter().map(|value| value.as_str().unwrap()).collect();
        assert_eq!(actual, expected);
    }
    if let Some(sections) = expect["raw_sections"].as_array() {
        let actual: Vec<&str> = subtitles
            .header
            .raw_sections
            .iter()
            .map(|section| section.name.as_str())
            .collect();
        let expected: Vec<&str> = sections
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(actual, expected);
    }
    if let Some(blocks) = expect["blocks"].as_array() {
        let actual: Vec<&str> = subtitles
            .header
            .blocks
            .iter()
            .map(|block| match block.kind {
                VttBlockKind::Style => "STYLE",
                VttBlockKind::Region => "REGION",
                VttBlockKind::Note => "NOTE",
            })
            .collect();
        let expected: Vec<&str> = blocks.iter().map(|value| value.as_str().unwrap()).collect();
        assert_eq!(actual, expected);
    }
    if let Some(signature) = expect["signature"].as_str() {
        assert_eq!(subtitles.header.vtt_signature.as_deref(), Some(signature));
    }
    if let Some(lines) = expect["header_lines"].as_array() {
        let expected: Vec<&str> = lines.iter().map(|value| value.as_str().unwrap()).collect();
        let actual: Vec<&str> = subtitles
            .header
            .vtt_header_lines
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(actual, expected);
    }
    if let Some(fields) = expect["ass_fields"].as_array() {
        for (cue, expected) in subtitles.cues.iter().zip(fields) {
            let actual = cue.ass.as_ref().unwrap();
            if let Some(layer) = expected["layer"].as_i64() {
                assert_eq!(actual.layer, layer as i32);
            }
            if let Some(marked) = expected["marked"].as_bool() {
                assert_eq!(actual.marked, marked);
            }
            if let Some(style) = expected["style"].as_str() {
                assert_eq!(actual.style, style);
            }
            if let Some(name) = expected["name"].as_str() {
                assert_eq!(actual.name, name);
            }
            if let Some(margin) = expected["margin_l"].as_i64() {
                assert_eq!(actual.margin_l, margin as i32);
            }
        }
    }
    assert_eq!(
        warning_set(subtitles),
        expected_warning_set(expect),
        "{}",
        record["id"]
    );
}

fn assert_error(record: &Value, error: &Error) {
    let expect = &record["expect"];
    assert_eq!(
        support::error_name(error),
        expect["error"].as_str().unwrap()
    );
    if let Some(line) = expect["line"].as_u64() {
        let line = physical_coordinate(record, "line", line);
        assert_eq!(support::error_line(error), Some(line), "{}", record["id"]);
    }
    if let Some(offset) = expect["offset"].as_u64() {
        let offset = physical_coordinate(record, "offset", offset);
        assert_eq!(
            support::error_offset(error),
            Some(offset),
            "{}",
            record["id"]
        );
    }
}

#[test]
fn replays_all_conformance_vectors() {
    let document = document();
    let vectors = document["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 98);
    for record in vectors {
        let input = bytes(record);
        assert_eq!(input.len(), record["len"].as_u64().unwrap() as usize);
        assert_eq!(support::sha256(&input), record["sha256"].as_str().unwrap());
        let parsed = parse(&input, format(record["format"].as_str().unwrap()));
        if record["kind"] == "error" {
            assert_error(record, &parsed.unwrap_err());
        } else {
            let subtitles = parsed.unwrap_or_else(|error| panic!("{}: {error}", record["id"]));
            assert_model(record, &subtitles);
            if record["expect"]["canonical"] == true {
                assert_eq!(subtitles.compose().0, input, "{}", record["id"]);
            }
        }
    }
}

#[test]
fn accepted_inputs_reach_a_compose_fixed_point() {
    for record in document()["vectors"].as_array().unwrap() {
        if record["kind"] == "error" || record["kind"] == "perf" {
            continue;
        }
        let source = parse(&bytes(record), format(record["format"].as_str().unwrap())).unwrap();
        let first = source.compose().0;
        let reparsed = parse(&first, source.format).unwrap();
        let second = reparsed.compose().0;
        assert_eq!(first, second, "{}", record["id"]);
        assert_eq!(source.cues.len(), reparsed.cues.len(), "{}", record["id"]);
    }
}

#[test]
fn conversions_parse_and_reach_a_fixed_point() {
    for record in document()["vectors"].as_array().unwrap() {
        if record["kind"] == "error" || record["kind"] == "perf" {
            continue;
        }
        let source = parse(&bytes(record), format(record["format"].as_str().unwrap())).unwrap();
        for target in [Format::Srt, Format::Vtt, Format::Ass] {
            if target == source.format {
                continue;
            }
            let converted = source.convert(target);
            assert_eq!(converted.cues.len(), source.cues.len(), "{}", record["id"]);
            let composed = converted.compose().0;
            let reparsed = parse(&composed, target)
                .unwrap_or_else(|error| panic!("{} to {target:?}: {error}", record["id"]));
            assert_eq!(reparsed.cues.len(), source.cues.len(), "{}", record["id"]);
            assert_eq!(reparsed.compose().0, composed, "{}", record["id"]);
        }
    }
}

#[test]
fn digit_run_overflow_is_a_typed_error() {
    named_error("SRT40", "TimeOutOfRange");
}

#[test]
fn checked_time_multiplication_reports_overflow() {
    named_error("SRT41", "TimeOutOfRange");
}

#[test]
fn ass_digit_run_overflow_is_a_typed_error() {
    named_error("ASS44", "TimeOutOfRange");
}

#[test]
fn blank_after_timing_keeps_the_empty_cue() {
    named_success("SRT16", 2, &["EmptyCue", "StrayText"]);
}

#[test]
fn digits_in_cue_text_do_not_start_a_block() {
    named_success("SRT15", 2, &["StrayText"]);
}

#[test]
fn utf16_typed_error() {
    named_error("SRT46", "Utf16Input");
}

fn record_by_id(id: &str) -> Value {
    document()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id)
        .unwrap()
        .clone()
}

fn named_error(id: &str, name: &str) {
    let record = record_by_id(id);
    let error = parse(&bytes(&record), format(record["format"].as_str().unwrap())).unwrap_err();
    assert_eq!(support::error_name(&error), name);
}

fn named_success(id: &str, cues: usize, warning_names: &[&str]) {
    let record = record_by_id(id);
    let subtitles = parse(&bytes(&record), format(record["format"].as_str().unwrap())).unwrap();
    assert_eq!(subtitles.cues.len(), cues);
    assert_eq!(
        warning_set(&subtitles),
        warning_names.iter().copied().collect()
    );
}

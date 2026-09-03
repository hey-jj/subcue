//! The streaming iterators yield the cues and warnings the whole-file
//! parsers produce, for every conformance record.

mod support;

use std::io::Cursor;

use serde_json::Value;
use subcue::{parse, AssCues, Cue, Error, Format, SrtCues, VttCues, Warning};

fn document() -> Value {
    serde_json::from_str(include_str!("../conformance-vectors.json")).unwrap()
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn format(value: &str) -> Format {
    match value {
        "srt" => Format::Srt,
        "vtt" => Format::Vtt,
        "ass" => Format::Ass,
        other => panic!("unknown format {other}"),
    }
}

struct Streamed {
    cues: Vec<Cue>,
    errors: Vec<Error>,
    warnings: Vec<Warning>,
}

fn stream(input: &[u8], format: Format) -> Result<Streamed, Error> {
    let mut cues = Vec::new();
    let mut errors = Vec::new();
    let mut collect = |item: Result<Cue, Error>| match item {
        Ok(cue) => cues.push(cue),
        Err(error) => errors.push(error),
    };
    let warnings = match format {
        Format::Srt => {
            let mut iterator = SrtCues::new(Cursor::new(input));
            (&mut iterator).for_each(&mut collect);
            iterator.warnings().to_vec()
        }
        Format::Vtt => {
            let mut iterator = VttCues::new(Cursor::new(input))?;
            (&mut iterator).for_each(&mut collect);
            iterator.warnings().to_vec()
        }
        Format::Ass => {
            let mut iterator = AssCues::new(Cursor::new(input))?;
            (&mut iterator).for_each(&mut collect);
            iterator.warnings().to_vec()
        }
    };
    Ok(Streamed {
        cues,
        errors,
        warnings,
    })
}

fn sorted_names(warnings: &[Warning]) -> Vec<String> {
    let mut names: Vec<String> = warnings
        .iter()
        .map(|warning| format!("{warning:?}"))
        .collect();
    names.sort();
    names
}

#[test]
fn streaming_matches_whole_file_parsing_on_every_accepted_record() {
    let document = document();
    let mut checked = 0;
    for record in document["vectors"].as_array().unwrap() {
        if record["kind"] == "error" || record["kind"] == "perf" {
            continue;
        }
        let id = record["id"].as_str().unwrap();
        let input = hex_bytes(record["hex"].as_str().unwrap());
        let format = format(record["format"].as_str().unwrap());
        let whole = parse(&input, format).unwrap();
        let streamed = stream(&input, format).unwrap_or_else(|error| panic!("{id}: {error}"));
        assert!(streamed.errors.is_empty(), "{id}: {:?}", streamed.errors);
        assert_eq!(streamed.cues, whole.cues, "{id}");
        assert_eq!(
            sorted_names(&streamed.warnings),
            sorted_names(&whole.warnings),
            "{id}"
        );
        if format == Format::Vtt {
            let iterator = VttCues::new(Cursor::new(&input[..])).unwrap();
            let mut iterator = iterator;
            iterator.by_ref().for_each(drop);
            assert_eq!(iterator.header(), &whole.header, "{id}");
        }
        checked += 1;
    }
    assert!(checked >= 70, "only {checked} records checked");
}

#[test]
fn streaming_surfaces_the_whole_file_error_on_every_error_record() {
    for record in document()["vectors"].as_array().unwrap() {
        if record["kind"] != "error" {
            continue;
        }
        let id = record["id"].as_str().unwrap();
        let input = hex_bytes(record["hex"].as_str().unwrap());
        let format = format(record["format"].as_str().unwrap());
        let expected = parse(&input, format).unwrap_err();
        let actual = match stream(&input, format) {
            Ok(streamed) => streamed
                .errors
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("{id}: streaming yielded no error")),
            Err(error) => error,
        };
        assert_eq!(
            support::error_name(&actual),
            support::error_name(&expected),
            "{id}"
        );
        assert_eq!(
            support::error_line(&actual),
            support::error_line(&expected),
            "{id}"
        );
        assert_eq!(
            support::error_offset(&actual),
            support::error_offset(&expected),
            "{id}"
        );
    }
}

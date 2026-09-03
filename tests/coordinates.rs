//! Error coordinates are physical: byte offsets count from the first input
//! byte and line numbers count physical lines after byte order mark removal.

use std::io::Cursor;

use subcue::{parse, parse_ass, parse_srt, parse_vtt, AssCues, Error, Format, SrtCues, VttCues};

const BOM: &[u8] = b"\xef\xbb\xbf";

fn offset_of(error: Error) -> usize {
    match error {
        Error::InvalidUtf8 { offset } => offset,
        other => panic!("expected InvalidUtf8, got {other:?}"),
    }
}

fn line_of(error: Error) -> usize {
    match error {
        Error::InvalidTimestamp { line } => line,
        other => panic!("expected InvalidTimestamp, got {other:?}"),
    }
}

fn bad_byte_at(input: &[u8]) -> usize {
    input.iter().position(|byte| *byte == 0xff).unwrap()
}

#[test]
fn invalid_utf8_at_file_start_is_offset_zero() {
    for format in [Format::Srt, Format::Vtt, Format::Ass] {
        assert_eq!(
            offset_of(parse(b"\xff", format).unwrap_err()),
            0,
            "{format:?}"
        );
    }
}

#[test]
fn invalid_utf8_after_a_bom_counts_the_bom_bytes() {
    let srt = [BOM, b"1\n\xff"].concat();
    assert_eq!(offset_of(parse_srt(&srt).unwrap_err()), bad_byte_at(&srt));
    let vtt = [BOM, b"WEBVTT\n\n\xff"].concat();
    assert_eq!(offset_of(parse_vtt(&vtt).unwrap_err()), bad_byte_at(&vtt));
    let ass = [BOM, b"[Script Info]\n\xff"].concat();
    assert_eq!(offset_of(parse_ass(&ass).unwrap_err()), bad_byte_at(&ass));
}

#[test]
fn invalid_utf8_after_crlf_counts_both_ending_bytes() {
    let srt = b"1\r\n00:00:01,000 --> 00:00:02,000\r\n\xff";
    assert_eq!(offset_of(parse_srt(srt).unwrap_err()), bad_byte_at(srt));
    let vtt = b"WEBVTT\r\n\r\n\xff";
    assert_eq!(offset_of(parse_vtt(vtt).unwrap_err()), bad_byte_at(vtt));
}

#[test]
fn invalid_utf8_inside_a_multibyte_line_is_the_byte_position() {
    let srt = b"1\n00:00:01,000 --> 00:00:02,000\nh\xc3\xa9llo \xe2\x82\xac \xff byte\n";
    assert_eq!(offset_of(parse_srt(srt).unwrap_err()), bad_byte_at(srt));
    let ass = b"[Script Info]\nTitle: h\xc3\xa9\xff\n";
    assert_eq!(offset_of(parse_ass(ass).unwrap_err()), bad_byte_at(ass));
}

#[test]
fn streaming_reports_the_same_offsets_as_whole_file_parsing() {
    let srt = b"1\n00:00:01,000 --> 00:00:02,000\nh\xc3\xa9 \xff\n";
    let whole = offset_of(parse_srt(srt).unwrap_err());
    let streamed = SrtCues::new(Cursor::new(&srt[..]))
        .find_map(|item| item.err())
        .unwrap();
    assert_eq!(offset_of(streamed), whole);
    assert_eq!(whole, bad_byte_at(srt));

    let vtt = [
        BOM,
        b"WEBVTT\r\n\r\n00:00:01.000 --> 00:00:02.000\r\nok \xff\r\n",
    ]
    .concat();
    let whole = offset_of(parse_vtt(&vtt).unwrap_err());
    let streamed = VttCues::new(Cursor::new(&vtt[..]))
        .unwrap()
        .find_map(|item| item.err())
        .unwrap();
    assert_eq!(offset_of(streamed), whole);
    assert_eq!(whole, bad_byte_at(&vtt));
}

#[test]
fn ass_timestamp_errors_name_the_physical_dialogue_line() {
    let plain = b"[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:xx.00,0:00:02.00,Default,,0,0,0,,bad\n";
    assert_eq!(line_of(parse_ass(plain).unwrap_err()), 3);
    let streamed = AssCues::new(Cursor::new(&plain[..]))
        .unwrap()
        .find_map(|item| item.err())
        .unwrap();
    assert_eq!(line_of(streamed), 3);

    let decorated = [
        BOM,
        b"[Script Info]\r\nTitle: t\r\n\r\n[Events]\r\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\r\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,ok\r\nDialogue: 0,0:00:xx.00,0:00:02.00,Default,,0,0,0,,bad\r\n",
    ]
    .concat();
    assert_eq!(line_of(parse_ass(&decorated).unwrap_err()), 7);
    let streamed = AssCues::new(Cursor::new(&decorated[..]))
        .unwrap()
        .find_map(|item| item.err())
        .unwrap();
    assert_eq!(line_of(streamed), 7);
}

#[test]
fn srt_and_vtt_timestamp_errors_name_the_physical_line() {
    let srt = [BOM, b"1\r\n00:00:0a,000 --> 00:00:02,000\r\n"].concat();
    assert_eq!(line_of(parse_srt(&srt).unwrap_err()), 2);
    let vtt = b"WEBVTT\nKind: captions\n\nid\n00:00:0a.000 --> 00:00:02.000\n";
    assert_eq!(line_of(parse_vtt(vtt).unwrap_err()), 5);
    let streamed = VttCues::new(Cursor::new(&vtt[..]))
        .unwrap()
        .find_map(|item| item.err())
        .unwrap();
    assert_eq!(line_of(streamed), 5);
}

//! A bare `<` in cue text does not hide the next tag, and a WebVTT timing
//! line inside cue text starts a new cue.

use std::io::Cursor;

use subcue::{parse, parse_srt, parse_vtt, Cue, Error, Format, VttCues, Warning};

fn srt_cue(text: &str) -> Vec<u8> {
    format!("1\n00:00:01,000 --> 00:00:02,000\n{text}\n\n").into_bytes()
}

fn fixed_point(subtitles: &subcue::Subtitles) {
    let composed = subtitles.compose().0;
    let reparsed = parse(&composed, subtitles.format).unwrap();
    assert_eq!(reparsed.cues, subtitles.cues);
    assert_eq!(reparsed.compose().0, composed);
}

#[test]
fn a_bare_less_than_keeps_the_following_tags() {
    let cases = [
        (
            "fish & chips < 3 <b>ok</b> &amp;",
            "fish & chips < 3 <b>ok</b> &amp;",
            "fish & chips < 3 {\\b1}ok{\\b0} &amp;",
        ),
        (
            "I <3 you <i>so</i> much",
            "I <3 you <i>so</i> much",
            "I <3 you {\\i1}so{\\i0} much",
        ),
        (
            "no open</b> here <b>on",
            "no open here <b>on</b>",
            "no open here {\\b1}on{\\b0}",
        ),
    ];
    for (input, vtt_text, ass_text) in cases {
        let source = parse_srt(&srt_cue(input)).unwrap();
        let vtt = source.convert(Format::Vtt);
        assert_eq!(vtt.cues[0].text, vtt_text, "{input}");
        assert!(vtt.warnings.is_empty(), "{input}: {:?}", vtt.warnings);
        fixed_point(&vtt);
        let ass = source.convert(Format::Ass);
        assert_eq!(ass.cues[0].text, ass_text, "{input}");
        assert!(ass.warnings.is_empty(), "{input}: {:?}", ass.warnings);
        fixed_point(&ass);
    }
}

#[test]
fn a_vtt_timing_line_inside_cue_text_starts_a_new_cue() {
    let input = b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nx\n00:00:03.000 --> 00:00:04.000\ny\n\n";
    let whole = parse_vtt(input).unwrap();
    let texts: Vec<&str> = whole.cues.iter().map(|cue| cue.text.as_str()).collect();
    assert_eq!(texts, ["x", "y"]);
    assert_eq!(whole.cues[1].start.0, 3000);
    assert_eq!(whole.warnings, vec![Warning::MissingBlankLine { line: 5 }]);
    let mut cues = VttCues::new(Cursor::new(&input[..])).unwrap();
    let streamed: Vec<Cue> = cues.by_ref().map(Result::unwrap).collect();
    assert_eq!(streamed, whole.cues);
    assert_eq!(cues.warnings(), &whole.warnings[..]);

    let with_id =
        b"WEBVTT\n\nfirst\n00:00:01.000 --> 00:00:02.000\nx\n00:00:03.000 --> 00:00:04.000\ny\nz\n";
    let whole = parse_vtt(with_id).unwrap();
    let ids: Vec<Option<&str>> = whole.cues.iter().map(|cue| cue.id.as_deref()).collect();
    assert_eq!(ids, [Some("first"), None]);
    assert_eq!(whole.cues[1].text, "y\nz");
    let mut cues = VttCues::new(Cursor::new(&with_id[..])).unwrap();
    let streamed: Vec<Cue> = cues.by_ref().map(Result::unwrap).collect();
    assert_eq!(streamed, whole.cues);
    fixed_point(&whole);
}

#[test]
fn a_non_parsing_arrow_line_inside_vtt_text_stays_text() {
    let input = b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\ngo --> there\nstill\n\n";
    let whole = parse_vtt(input).unwrap();
    assert_eq!(whole.cues.len(), 1);
    assert_eq!(whole.cues[0].text, "go --> there\nstill");
    assert!(whole.warnings.is_empty());
    let streamed: Vec<Cue> = VttCues::new(Cursor::new(&input[..]))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(streamed, whole.cues);
}

#[test]
fn a_bad_timing_line_after_text_is_still_an_error_in_block_position() {
    let input = b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nx\n\n00:00:0a.000 --> 00:00:04.000\ny\n";
    assert!(matches!(
        parse_vtt(input),
        Err(Error::InvalidTimestamp { line: 6 })
    ));
    let mut cues = VttCues::new(Cursor::new(&input[..])).unwrap();
    assert_eq!(cues.next().unwrap().unwrap().text, "x");
    assert!(matches!(
        cues.next(),
        Some(Err(Error::InvalidTimestamp { line: 6 }))
    ));
}

//! Malformed and edited inputs: no panic, no silent loss, and every
//! accepted input composes to a fixed point.

use std::io::Cursor;

use subcue::{
    parse, parse_ass, parse_srt, parse_vtt, AssCues, Cue, Error, Format, Header, SrtCues,
    Subtitles, Time, VttBlock, VttBlockKind, VttCues, Warning,
};

const ASS_HEAD: &str = "[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";

fn srt_cue(text: &str) -> Vec<u8> {
    format!("1\n00:00:01,000 --> 00:00:02,000\n{text}\n\n").into_bytes()
}

fn ass_cue(text: &str) -> Vec<u8> {
    format!("{ASS_HEAD}Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{text}\n").into_bytes()
}

fn texts(subtitles: &Subtitles) -> Vec<&str> {
    subtitles.cues.iter().map(|cue| cue.text.as_str()).collect()
}

fn names(subtitles: &Subtitles) -> Vec<&'static str> {
    subtitles
        .warnings
        .iter()
        .map(|warning| match warning {
            Warning::StrayText { .. } => "StrayText",
            Warning::MissingIndex { .. } => "MissingIndex",
            Warning::MissingBlankLine { .. } => "MissingBlankLine",
            Warning::LossyTag { .. } => "LossyTag",
            Warning::NoCues => "NoCues",
            _ => "other",
        })
        .collect()
}

fn fixed_point(subtitles: &Subtitles) {
    let first = subtitles.compose().0;
    let reparsed = parse(&first, subtitles.format).unwrap();
    assert_eq!(reparsed.cues, subtitles.cues);
    assert_eq!(reparsed.compose().0, first);
}

#[test]
fn non_ascii_bytes_inside_colour_values_never_panic() {
    for text in [
        "{\\c&H0é000&}x",
        "{\\c&Habé00000&}x",
        "{\\1c&Hé&}x",
        "{\\c&H日本語&}x",
    ] {
        let converted = parse_ass(&ass_cue(text)).unwrap().convert(Format::Srt);
        assert_eq!(converted.cues[0].text, "x", "{text}");
        assert!(
            matches!(converted.warnings[0], Warning::LossyTag { cue: 1, .. }),
            "{text}"
        );
    }
    for text in [
        "<font color=\"#0é000\">x</font>",
        "<font color=\"#é0000\">x</font>",
        "<font color=#0é000>x</font>",
        "<font color=\"#ab日\">x</font>",
    ] {
        let converted = parse_srt(&srt_cue(text)).unwrap().convert(Format::Ass);
        assert_eq!(converted.cues[0].text, "x", "{text}");
        assert!(
            matches!(converted.warnings[0], Warning::LossyTag { cue: 1, .. }),
            "{text}"
        );
    }
}

#[test]
fn an_arrow_inside_srt_text_is_text() {
    let input =
        b"1\n00:00:01,000 --> 00:00:02,000\ngo --> there\n\n2\n00:00:03,000 --> 00:00:04,000\nok\n";
    let whole = parse_srt(input).unwrap();
    assert_eq!(texts(&whole), ["go --> there", "ok"]);
    assert!(whole.warnings.is_empty(), "{:?}", whole.warnings);
    let streamed: Vec<Cue> = SrtCues::new(Cursor::new(&input[..]))
        .map(Result::unwrap)
        .collect();
    assert_eq!(streamed, whole.cues);
    let minimal = parse_srt(b"1\n00:00:01,000 --> 00:00:02,000\ngo --> there\n").unwrap();
    assert_eq!(texts(&minimal), ["go --> there"]);
    fixed_point(&minimal);
    let from_ass = parse_ass(&ass_cue("go --> there")).unwrap();
    for target in [Format::Srt, Format::Vtt] {
        fixed_point(&from_ass.convert(target));
    }
}

#[test]
fn a_parseable_timing_line_still_starts_a_block_in_text_position() {
    let input = b"1\n00:00:01,000 --> 00:00:02,000\nfirst\n00:00:03,000 --> 00:00:04,000\nsecond\n";
    let whole = parse_srt(input).unwrap();
    assert_eq!(texts(&whole), ["first", "second"]);
    let mut warned = names(&whole);
    warned.sort_unstable();
    assert_eq!(warned, ["MissingBlankLine", "MissingIndex"]);
    let streamed: Vec<Cue> = SrtCues::new(Cursor::new(&input[..]))
        .map(Result::unwrap)
        .collect();
    assert_eq!(streamed, whole.cues);
    let bad = parse_srt(b"1\n00:00:0a,000 --> 00:00:02,000\nbad\n").unwrap_err();
    assert!(matches!(bad, Error::InvalidTimestamp { line: 2 }));
    let empty_then_next =
        parse_srt(b"1\n00:00:01,000 --> 00:00:02,000\n2\n00:00:03,000 --> 00:00:04,000\nsecond\n")
            .unwrap();
    assert_eq!(texts(&empty_then_next), ["", "second"]);
}

#[test]
fn duplicate_ass_sections_merge_and_compose_once() {
    let mut input = ASS_HEAD.to_owned();
    for index in 0..3 {
        input.push_str(&format!(
            "Dialogue: 0,0:00:0{index}.00,0:00:05.00,Default,,0,0,0,,a{index}\n"
        ));
    }
    input.push_str("\n[V4+ Styles]\nFormat: Name, Fontname\nStyle: Second,Arial\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    for index in 3..6 {
        input.push_str(&format!(
            "Dialogue: 0,0:00:0{index}.00,0:00:09.00,Default,,0,0,0,,a{index}\n"
        ));
    }
    let parsed = parse_ass(input.as_bytes()).unwrap();
    assert_eq!(parsed.cues.len(), 6);
    assert_eq!(parsed.header.styles.len(), 2);
    let composed = parsed.compose().0;
    let text = String::from_utf8(composed.clone()).unwrap();
    assert_eq!(text.matches("[Events]").count(), 1);
    assert_eq!(text.matches("[V4+ Styles]").count(), 1);
    let reparsed = parse_ass(&composed).unwrap();
    assert_eq!(reparsed.cues, parsed.cues);
    assert_eq!(reparsed.header.styles, parsed.header.styles);
    assert_eq!(reparsed.compose().0, composed);
}

#[test]
fn raw_sections_keep_their_position_and_header_supports_struct_update() {
    let input = b"[Script Info]\nTitle: t\n\n[Aegisub Project Garbage]\nAudio File: a.wav\n\n[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Arial\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,x\n\n[Fonts]\nfontname: a.ttf\n";
    let parsed = parse_ass(input).unwrap();
    let text = String::from_utf8(parsed.compose().0).unwrap();
    let order: Vec<&str> = text.lines().filter(|line| line.starts_with('[')).collect();
    assert_eq!(
        order,
        [
            "[Script Info]",
            "[Aegisub Project Garbage]",
            "[V4+ Styles]",
            "[Events]",
            "[Fonts]"
        ]
    );
    fixed_point(&parsed);
    let header = Header {
        blocks: vec![VttBlock {
            kind: VttBlockKind::Note,
            raw: "NOTE hi".to_owned(),
            cue_index: 0,
        }],
        ..Header::default()
    };
    assert_eq!(header.blocks.len(), 1);
}

#[test]
fn ass_to_vtt_escapes_plain_text_only_and_the_voice_name() {
    let converted = parse_ass(&ass_cue("{\\b1}Hi{\\b0} & <you>"))
        .unwrap()
        .convert(Format::Vtt);
    assert_eq!(converted.cues[0].text, "<b>Hi</b> &amp; &lt;you&gt;");
    assert!(converted.warnings.is_empty());
    let named = ass_cue("Hi").replace_name("<b>Bob & Co");
    let converted = parse_ass(&named).unwrap().convert(Format::Vtt);
    assert_eq!(converted.cues[0].text, "<v &lt;b&gt;Bob &amp; Co>Hi");
    fixed_point(&converted);
}

trait ReplaceName {
    fn replace_name(&self, name: &str) -> Vec<u8>;
}

impl ReplaceName for Vec<u8> {
    fn replace_name(&self, name: &str) -> Vec<u8> {
        String::from_utf8(self.clone())
            .unwrap()
            .replace("Default,,0,0,0,,", &format!("Default,{name},0,0,0,,"))
            .into_bytes()
    }
}

#[test]
fn converted_line_breaks_never_produce_an_empty_line() {
    for text in ["a\\N\\Nb", "\\Na", "a\\N", "\\N\\N", "a\\N\\N\\Nb"] {
        let source = parse_ass(&ass_cue(text)).unwrap();
        for target in [Format::Srt, Format::Vtt] {
            let converted = source.convert(target);
            let composed = converted.compose().0;
            let reparsed = parse(&composed, target)
                .unwrap_or_else(|error| panic!("{text} to {target:?}: {error}"));
            assert_eq!(reparsed.cues, converted.cues, "{text} to {target:?}");
            let lines = converted.cues[0].text.split('\n').count();
            assert_eq!(
                lines,
                text.matches("\\N").count() + 1,
                "{text} to {target:?}"
            );
        }
    }
    let srt = parse_srt(b"1\n00:00:01,000 --> 00:00:02,000\ntext1\n\n2\n\n2\n00:00:03,000 --> 00:00:04,000\ntext2\n").unwrap();
    assert_eq!(srt.cues[0].text, "text1\n\n2");
    let vtt = srt.convert(Format::Vtt);
    assert_eq!(vtt.cues[0].text, "text1\n \n2");
    fixed_point(&vtt);
}

#[test]
fn events_and_blocks_survive_cue_removal() {
    let mut input = ASS_HEAD.to_owned();
    input.push_str("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,d1\nDialogue: 0,0:00:02.00,0:00:03.00,Default,,0,0,0,,d2\nComment: 0,0:00:00.00,0:00:00.00,Default,,0,0,0,,note\nDialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,d3\nSound: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,beep.wav\n");
    let mut parsed = parse_ass(input.as_bytes()).unwrap();
    parsed.cues.retain(|cue| cue.start.0 >= 3000);
    let composed = parsed.compose().0;
    let rows: Vec<String> = String::from_utf8(composed.clone())
        .unwrap()
        .lines()
        .filter(|line| line.contains(",0:0"))
        .map(|line| line.rsplit(',').next().unwrap().to_owned())
        .collect();
    assert_eq!(rows, ["d3", "note", "beep.wav"]);
    let reparsed = parse_ass(&composed).unwrap();
    assert_eq!(reparsed.events.len(), 2);
    parsed.cues.clear();
    let reparsed = parse_ass(&parsed.compose().0).unwrap();
    assert_eq!(reparsed.events.len(), 2);

    let mut vtt = parse_vtt(b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\na\n\nNOTE keep me\n\n00:00:03.000 --> 00:00:04.000\nb\n").unwrap();
    vtt.cues.pop();
    let composed = String::from_utf8(vtt.compose().0).unwrap();
    assert_eq!(
        composed,
        "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\na\n\nNOTE keep me\n\n"
    );
    vtt.cues.clear();
    assert_eq!(
        String::from_utf8(vtt.compose().0).unwrap(),
        "WEBVTT\n\nNOTE keep me\n\n"
    );
}

#[test]
fn missing_arrow_needs_a_line_that_reads_as_a_time() {
    let cases: [&[u8]; 3] = [
        b"2nd: yes, ok\n\n1\n00:00:01,000 --> 00:00:02,000\nhi\n",
        b"12:30, meet me\n\n1\n00:00:01,000 --> 00:00:02,000\nhi\n",
        b"1\n12:30, meet me\n\n1\n00:00:01,000 --> 00:00:02,000\nhi\n",
    ];
    for input in cases {
        let whole = parse_srt(input).unwrap();
        assert_eq!(texts(&whole), ["hi"]);
        assert_eq!(names(&whole), ["StrayText"]);
        let mut cues = SrtCues::new(Cursor::new(input));
        let streamed: Vec<Cue> = cues.by_ref().map(Result::unwrap).collect();
        assert_eq!(streamed, whole.cues);
        assert_eq!(cues.warnings().len(), 1);
    }
    for input in [
        &b"1\n00:00:01,000 -> 00:00:02,000\nsingle dash\n"[..],
        b"00:00:01,000 00:00:02,000\nhi\n",
    ] {
        assert!(matches!(parse_srt(input), Err(Error::MissingArrow { .. })));
        assert!(matches!(
            SrtCues::new(Cursor::new(input)).next(),
            Some(Err(Error::MissingArrow { .. }))
        ));
    }
}

#[test]
fn a_vtt_block_without_a_timing_line_is_discarded_with_a_warning() {
    let cases: [(&[u8], usize); 3] = [
        (
            b"WEBVTT\n\nKind: captions\nLanguage: en\n\n00:00:03.000 --> 00:00:04.000\nyo\n",
            3,
        ),
        (
            b"WEBVTT\n\nnote lower\n\n00:00:03.000 --> 00:00:04.000\nyo\n",
            3,
        ),
        (
            b"WEBVTT\n\n00:00:03.000 --> 00:00:04.000\nyo\n\ntrailing junk\n",
            6,
        ),
    ];
    for (input, line) in cases {
        let whole = parse_vtt(input).unwrap();
        assert_eq!(texts(&whole), ["yo"]);
        assert_eq!(whole.warnings, vec![Warning::StrayText { line }]);
        let mut cues = VttCues::new(Cursor::new(input)).unwrap();
        let streamed: Vec<Cue> = cues.by_ref().map(Result::unwrap).collect();
        assert_eq!(streamed, whole.cues);
        assert_eq!(cues.warnings(), &whole.warnings[..]);
    }
    assert!(matches!(
        parse_vtt(b"WEBVTT\n\n00:00:0x.000 --> 00:00:02.000\nbad\n"),
        Err(Error::InvalidTimestamp { line: 3 })
    ));
}

#[test]
fn negative_times_compose_as_zero() {
    let subtitles = Subtitles {
        format: Format::Srt,
        header: Header::default(),
        cues: vec![Cue {
            index: None,
            id: None,
            start: Time(-1500),
            end: Time(-500),
            text: "x".to_owned(),
            settings: None,
            ass: None,
        }],
        events: Vec::new(),
        warnings: Vec::new(),
    };
    assert_eq!(
        subtitles.compose().0,
        b"1\n00:00:00,000 --> 00:00:00,000\nx\n\n"
    );
}

#[test]
fn a_line_feed_in_ass_text_composes_as_a_break() {
    let mut parsed = parse_ass(&ass_cue("a")).unwrap();
    parsed.cues[0].text = "line1\nline2".to_owned();
    let composed = parsed.compose().0;
    let reparsed = parse_ass(&composed).unwrap();
    assert_eq!(texts(&reparsed), ["line1\\Nline2"]);
    assert_eq!(reparsed.compose().0, composed);
}

#[test]
fn streaming_reads_a_styles_section_that_follows_the_events() {
    let input = b"[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,a\n\n[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Arial\n";
    let mut cues = AssCues::new(Cursor::new(&input[..])).unwrap();
    let streamed: Vec<Cue> = cues.by_ref().map(Result::unwrap).collect();
    let whole = parse_ass(input).unwrap();
    assert_eq!(streamed, whole.cues);
    assert_eq!(cues.header().styles, whole.header.styles);
    assert!(!cues.warnings().contains(&Warning::MissingStyles));
    assert!(!whole.warnings.contains(&Warning::MissingStyles));
}

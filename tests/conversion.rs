//! Conversions go through the inline span model and round-trip through it.

use subcue::text::{self, Rgb, Span};
use subcue::{parse_ass, parse_srt, parse_vtt, Format, Warning};

fn text(value: &str) -> Span {
    Span::Text(value.to_owned())
}

#[test]
fn srt_formatting_survives_a_trip_through_ass() {
    let source = parse_srt(
        b"1\n00:00:01,000 --> 00:00:02,000\n<b>a <i>b</i></b>\n<font color=\"#ff0000\">c</font>\n\n",
    )
    .unwrap();
    let ass = source.convert(Format::Ass);
    assert_eq!(
        ass.cues[0].text,
        "{\\b1}a {\\i1}b{\\i0}{\\b0}\\N{\\c&H0000FF&}c{\\c}"
    );
    assert!(ass.warnings.is_empty(), "{:?}", ass.warnings);
    let back = ass.convert(Format::Srt);
    assert_eq!(back.cues[0].text, source.cues[0].text);
    assert_eq!(
        text::parse(&back.cues[0].text, Format::Srt).spans,
        text::parse(&source.cues[0].text, Format::Srt).spans
    );
}

#[test]
fn crossing_tags_are_normalized_to_a_tree() {
    let source = parse_srt(b"1\n00:00:01,000 --> 00:00:02,000\n<b><i>x</b>y</i>\n\n").unwrap();
    let vtt = source.convert(Format::Vtt);
    assert_eq!(vtt.cues[0].text, "<b><i>x</i></b><i>y</i>");
    assert_eq!(
        text::parse(&vtt.cues[0].text, Format::Vtt).spans,
        vec![
            Span::Bold(vec![Span::Italic(vec![text("x")])]),
            Span::Italic(vec![text("y")]),
        ]
    );
}

#[test]
fn vtt_voice_and_class_tags_take_their_documented_paths() {
    let source =
        parse_vtt(b"WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<v Bob><c.x>hi</c> &amp; bye\n\n")
            .unwrap();
    let ass = source.convert(Format::Ass);
    assert_eq!(ass.cues[0].text, "hi & bye");
    assert_eq!(ass.cues[0].ass.as_ref().unwrap().name, "Bob");
    assert_eq!(
        ass.warnings,
        vec![Warning::LossyTag {
            cue: 1,
            tag: "c.x".to_owned()
        }]
    );
    let srt = source.convert(Format::Srt);
    assert_eq!(srt.cues[0].text, "hi &amp; bye");
    assert_eq!(
        srt.warnings,
        vec![
            Warning::LossyTag {
                cue: 1,
                tag: "v Bob".to_owned()
            },
            Warning::LossyTag {
                cue: 1,
                tag: "c.x".to_owned()
            },
        ]
    );
}

#[test]
fn ass_overrides_map_to_settings_text_and_warnings() {
    let source = parse_ass(
        b"[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,Ann,0,0,0,,{\\an8\\pos(1,2)}a & b\\N<c>\n",
    )
    .unwrap();
    let vtt = source.convert(Format::Vtt);
    assert_eq!(
        vtt.cues[0].settings.as_deref(),
        Some("line:0% align:center")
    );
    assert_eq!(vtt.cues[0].text, "<v Ann>a &amp; b\n&lt;c&gt;");
    assert_eq!(
        vtt.warnings,
        vec![Warning::LossyTag {
            cue: 1,
            tag: "pos(1,2)".to_owned()
        }]
    );
    let srt = source.convert(Format::Srt);
    assert_eq!(srt.cues[0].text, "a & b\n<c>");
    assert_eq!(srt.cues[0].settings, None);
    assert_eq!(
        srt.warnings,
        vec![
            Warning::LossyTag {
                cue: 1,
                tag: "an8".to_owned()
            },
            Warning::LossyTag {
                cue: 1,
                tag: "pos(1,2)".to_owned()
            },
        ]
    );
}

#[test]
fn spans_are_a_public_model_with_colour_values() {
    let parsed = text::parse("{\\c&H0080FF&}warm{\\c}", Format::Ass);
    assert_eq!(
        parsed.spans,
        vec![Span::Color(
            Rgb {
                red: 255,
                green: 128,
                blue: 0
            },
            vec![text("warm")]
        )]
    );
    let rendered = text::render(&parsed.spans, Format::Vtt);
    assert_eq!(rendered.text, "warm");
    assert_eq!(rendered.dropped, vec!["font"]);
}

#[test]
fn converted_text_with_repeated_breaks_reparses_to_the_same_cues() {
    let source = parse_ass(
        b"[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,\\Nfirst\\N\\Nsecond\\N\n",
    )
    .unwrap();
    for target in [Format::Srt, Format::Vtt] {
        let converted = source.convert(target);
        let composed = converted.compose().0;
        let reparsed = subcue::parse(&composed, target).unwrap();
        assert_eq!(reparsed.cues, converted.cues, "{target:?}");
        assert_eq!(reparsed.compose().0, composed, "{target:?}");
    }
}

use crate::model::{AssFields, Cue, Format, Header, Style, Subtitles, Warning};
use crate::text::{self, ParsedText, Span};

pub(crate) fn convert(source: &Subtitles, target: Format) -> Subtitles {
    if source.format == target {
        return source.clone();
    }
    match (source.format, target) {
        (Format::Srt, Format::Vtt) => srt_to_vtt(source),
        (Format::Srt, Format::Ass) => text_to_ass(source, Format::Srt),
        (Format::Vtt, Format::Srt) => vtt_to_srt(source),
        (Format::Vtt, Format::Ass) => text_to_ass(source, Format::Vtt),
        (Format::Ass, Format::Srt) => ass_to_text(source, Format::Srt),
        (Format::Ass, Format::Vtt) => ass_to_text(source, Format::Vtt),
        _ => source.clone(),
    }
}

fn srt_to_vtt(source: &Subtitles) -> Subtitles {
    let mut warnings = Vec::new();
    let cues = source
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| {
            let number = index + 1;
            let mut settings = None;
            let mut raw = cue.text.as_str();
            let stripped;
            if let Some((alignment, rest)) = take_alignment(raw) {
                stripped = rest;
                raw = &stripped;
                settings = alignment_settings(alignment).map(str::to_owned);
            }
            let parsed = text::parse(raw, Format::Srt);
            let text = render(&parsed.spans, Format::Vtt, number, &mut warnings);
            if cue.settings.is_some() {
                warnings.push(Warning::DroppedSettings { cue: number });
            }
            Cue {
                index: None,
                id: cue.index.map(|value| value.to_string()),
                start: cue.start,
                end: cue.end,
                text,
                settings,
                ass: None,
            }
        })
        .collect();
    Subtitles {
        format: Format::Vtt,
        header: Header::default(),
        cues,
        events: Vec::new(),
        warnings,
    }
}

fn vtt_to_srt(source: &Subtitles) -> Subtitles {
    let mut warnings = Vec::new();
    for block in &source.header.blocks {
        push_warning_once(&mut warnings, Warning::DroppedBlock { kind: block.kind });
    }
    let cues = source
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| {
            let number = index + 1;
            let parsed_index = cue.id.as_deref().and_then(|id| id.parse::<u32>().ok());
            if cue.id.is_some() && parsed_index.is_none() {
                warnings.push(Warning::DroppedId { cue: number });
            }
            if cue.settings.is_some() {
                warnings.push(Warning::DroppedSettings { cue: number });
            }
            let parsed = text::parse(&cue.text, Format::Vtt);
            if let Some(voice) = &parsed.voice {
                lossy(&mut warnings, number, &format!("v {voice}"));
            }
            Cue {
                index: parsed_index.or_else(|| u32::try_from(number).ok()),
                id: None,
                start: cue.start,
                end: cue.end,
                text: render(&parsed.spans, Format::Srt, number, &mut warnings),
                settings: None,
                ass: None,
            }
        })
        .collect();
    Subtitles {
        format: Format::Srt,
        header: Header::default(),
        cues,
        events: Vec::new(),
        warnings,
    }
}

fn text_to_ass(source: &Subtitles, from: Format) -> Subtitles {
    let mut warnings = Vec::new();
    if from == Format::Vtt {
        for block in &source.header.blocks {
            push_warning_once(&mut warnings, Warning::DroppedBlock { kind: block.kind });
        }
    }
    let cues = source
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| {
            let number = index + 1;
            let ParsedText {
                mut spans, voice, ..
            } = text::parse(&cue.text, from);
            if from == Format::Vtt {
                map_text(&mut spans, &decode_entities);
                if cue.id.is_some() {
                    warnings.push(Warning::DroppedId { cue: number });
                }
            }
            let text = render(&spans, Format::Ass, number, &mut warnings);
            if cue.settings.is_some() {
                warnings.push(Warning::DroppedSettings { cue: number });
            }
            if cue.start.0 % 10 != 0 || cue.end.0 % 10 != 0 {
                warnings.push(Warning::TimePrecisionLost { cue: number });
            }
            Cue {
                index: None,
                id: None,
                start: cue.start,
                end: cue.end,
                text,
                settings: None,
                ass: Some(AssFields {
                    name: voice.unwrap_or_default(),
                    ..AssFields::default()
                }),
            }
        })
        .collect();
    let mut header = Header::default();
    header
        .script_info
        .push(("ScriptType".to_owned(), "v4.00+".to_owned()));
    header.styles.push(Style::default());
    Subtitles {
        format: Format::Ass,
        header,
        cues,
        events: Vec::new(),
        warnings,
    }
}

fn ass_to_text(source: &Subtitles, target: Format) -> Subtitles {
    let mut warnings = Vec::new();
    for event in &source.events {
        push_warning_once(
            &mut warnings,
            Warning::DroppedEvent {
                index: event.event_index,
            },
        );
    }
    let cues = source
        .cues
        .iter()
        .enumerate()
        .map(|(index, cue)| {
            let number = index + 1;
            let ParsedText {
                mut spans,
                alignment,
                ..
            } = text::parse(&cue.text, Format::Ass);
            let name = cue
                .ass
                .as_ref()
                .map(|fields| fields.name.as_str())
                .unwrap_or("");
            let mut settings = None;
            match (target, alignment) {
                (Format::Srt, Some(alignment)) => {
                    lossy(&mut warnings, number, &format!("an{alignment}"));
                }
                (Format::Vtt, Some(alignment)) => {
                    settings = alignment_settings(alignment).map(str::to_owned);
                }
                _ => {}
            }
            if target == Format::Vtt {
                map_text(&mut spans, &escape_vtt_text);
            }
            let mut text = render(&spans, target, number, &mut warnings);
            if target == Format::Vtt && !name.is_empty() {
                text = format!("<v {}>{text}", escape_vtt_text(name));
            }
            Cue {
                index: (target == Format::Srt)
                    .then(|| u32::try_from(number).ok())
                    .flatten(),
                id: None,
                start: cue.start,
                end: cue.end,
                text,
                settings,
                ass: None,
            }
        })
        .collect();
    Subtitles {
        format: target,
        header: Header::default(),
        cues,
        events: Vec::new(),
        warnings,
    }
}

fn render(spans: &[Span], target: Format, cue: usize, warnings: &mut Vec<Warning>) -> String {
    let rendered = text::render(spans, target);
    for tag in rendered.dropped {
        warnings.push(Warning::LossyTag { cue, tag });
    }
    rendered.text
}

fn map_text(spans: &mut [Span], map: &dyn Fn(&str) -> String) {
    for span in spans {
        match span {
            Span::Text(text) => *text = map(text),
            Span::Bold(children)
            | Span::Italic(children)
            | Span::Underline(children)
            | Span::Strike(children)
            | Span::Color(_, children) => map_text(children, map),
            Span::LineBreak | Span::Raw(_) => {}
        }
    }
}

fn take_alignment(input: &str) -> Option<(u8, String)> {
    for alignment in 1..=9 {
        let marker = format!("{{\\an{alignment}}}");
        if input.contains(&marker) {
            return Some((alignment, input.replacen(&marker, "", 1)));
        }
    }
    None
}

fn alignment_settings(alignment: u8) -> Option<&'static str> {
    match alignment {
        1 => Some("line:100% align:start"),
        2 => None,
        3 => Some("line:100% align:end"),
        4 => Some("line:50% align:start"),
        5 => Some("line:50% align:center"),
        6 => Some("line:50% align:end"),
        7 => Some("line:0% align:start"),
        8 => Some("line:0% align:center"),
        9 => Some("line:0% align:end"),
        _ => None,
    }
}

fn decode_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u{00a0}")
}

fn escape_vtt_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for character in input.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
    output
}

fn lossy(warnings: &mut Vec<Warning>, cue: usize, tag: &str) {
    warnings.push(Warning::LossyTag {
        cue,
        tag: tag.to_owned(),
    });
}

/// Dropped blocks and events come in file order and are pushed before the
/// first cue warning, so the scan stays within that short prefix.
fn push_warning_once(warnings: &mut Vec<Warning>, warning: Warning) {
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

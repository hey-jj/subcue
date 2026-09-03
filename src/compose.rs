use std::collections::HashSet;

use crate::model::{
    AssEvent, AssFields, ComposeOptions, EventKind, Format, Header, LineEnding, RawSection, Style,
    Subtitles, Time, VttBlock, Warning,
};

const STYLE_FORMAT_LINE: &str = "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding";
const EVENT_FORMAT_LINE: &str =
    "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text";

impl Subtitles {
    /// Composes canonical bytes with LF line endings and no byte order mark.
    pub fn compose(&self) -> (Vec<u8>, Vec<Warning>) {
        self.compose_with(&ComposeOptions::default())
    }

    /// Composes canonical bytes with the selected output options.
    ///
    /// Compose is total. A negative [`Time`] is written as zero, and a
    /// line feed inside ASS cue text is written as `\N`, because the output
    /// format cannot carry either value.
    pub fn compose_with(&self, options: &ComposeOptions) -> (Vec<u8>, Vec<Warning>) {
        let mut warnings: Vec<Warning> = self
            .warnings
            .iter()
            .filter(|warning| warning.is_output_warning())
            .cloned()
            .collect();
        let text = match self.format {
            Format::Srt => compose_srt(self),
            Format::Vtt => compose_vtt(self),
            Format::Ass => compose_ass(self, &mut warnings),
        };
        let mut bytes = if options.line_ending == LineEnding::CrLf {
            text.replace('\n', "\r\n").into_bytes()
        } else {
            text.into_bytes()
        };
        if options.bom {
            let mut prefixed = Vec::with_capacity(bytes.len() + 3);
            prefixed.extend_from_slice(&[0xef, 0xbb, 0xbf]);
            prefixed.append(&mut bytes);
            bytes = prefixed;
        }
        (bytes, warnings)
    }

    /// Converts this value to another supported subtitle format.
    pub fn convert(&self, to: Format) -> Subtitles {
        crate::convert::convert(self, to)
    }

    /// Rewrites SRT indices to one-based file order.
    pub fn renumber(&mut self) {
        if self.format == Format::Srt {
            for (position, cue) in self.cues.iter_mut().enumerate() {
                cue.index = u32::try_from(position + 1).ok();
            }
        }
    }
}

fn compose_srt(subtitles: &Subtitles) -> String {
    let mut output = String::new();
    for (position, cue) in subtitles.cues.iter().enumerate() {
        let index = cue
            .index
            .or_else(|| u32::try_from(position + 1).ok())
            .unwrap_or(u32::MAX);
        output.push_str(&index.to_string());
        output.push('\n');
        output.push_str(&format_srt_time(cue.start));
        output.push_str(" --> ");
        output.push_str(&format_srt_time(cue.end));
        if let Some(settings) = &cue.settings {
            if !settings.is_empty() {
                output.push(' ');
                output.push_str(settings);
            }
        }
        output.push('\n');
        if !cue.text.is_empty() {
            output.push_str(&cue.text);
            output.push('\n');
        }
        output.push('\n');
    }
    output
}

fn compose_vtt(subtitles: &Subtitles) -> String {
    let mut output = String::from("WEBVTT");
    if let Some(signature) = &subtitles.header.vtt_signature {
        if !signature.is_empty() {
            output.push(' ');
            output.push_str(signature);
        }
    }
    output.push('\n');
    for line in &subtitles.header.vtt_header_lines {
        output.push_str(line);
        output.push('\n');
    }
    output.push('\n');
    let mut blocks: Vec<&VttBlock> = subtitles.header.blocks.iter().collect();
    blocks.sort_by_key(|block| block.cue_index);
    let mut next_block = 0;
    for (cue_index, cue) in subtitles.cues.iter().enumerate() {
        while blocks
            .get(next_block)
            .is_some_and(|block| block.cue_index <= cue_index)
        {
            output.push_str(&blocks[next_block].raw);
            output.push_str("\n\n");
            next_block += 1;
        }
        if let Some(id) = &cue.id {
            output.push_str(id);
            output.push('\n');
        }
        output.push_str(&format_vtt_time(cue.start));
        output.push_str(" --> ");
        output.push_str(&format_vtt_time(cue.end));
        if let Some(settings) = &cue.settings {
            if !settings.is_empty() {
                output.push(' ');
                output.push_str(settings);
            }
        }
        output.push('\n');
        if !cue.text.is_empty() {
            output.push_str(&cue.text);
            output.push('\n');
        }
        output.push('\n');
    }
    for block in &blocks[next_block..] {
        output.push_str(&block.raw);
        output.push_str("\n\n");
    }
    output
}

#[derive(Clone, Copy)]
enum Typed {
    ScriptInfo,
    Styles,
    Events,
}

/// Raw sections are written at their recorded position. The script
/// information, styles, and events sections fill the remaining positions in
/// that order, so a file whose raw sections sit between them keeps its
/// layout.
fn compose_ass(subtitles: &Subtitles, warnings: &mut Vec<Warning>) -> String {
    let header = &subtitles.header;
    let raw_holds_styles = header.raw_sections.iter().any(|section| {
        section
            .lines
            .iter()
            .any(|line| line.trim_start().starts_with("Style:"))
    });
    let mut typed = vec![Typed::ScriptInfo];
    if !(header.styles.is_empty() && raw_holds_styles) {
        typed.push(Typed::Styles);
    }
    typed.push(Typed::Events);
    let mut typed = typed.into_iter().peekable();
    let mut raws: Vec<&RawSection> = header.raw_sections.iter().collect();
    raws.sort_by_key(|section| section.position);
    let mut next_raw = 0;
    let mut position = 0;
    let mut output = String::new();
    while next_raw < raws.len() || typed.peek().is_some() {
        if position > 0 {
            while output.ends_with("\n\n\n") {
                output.pop();
            }
            if !output.ends_with("\n\n") {
                if !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push('\n');
            }
        }
        let raw_due = raws
            .get(next_raw)
            .is_some_and(|section| section.position <= position || typed.peek().is_none());
        if raw_due {
            write_raw_section(&mut output, raws[next_raw]);
            next_raw += 1;
        } else {
            match typed.next() {
                Some(Typed::ScriptInfo) => write_script_info(&mut output, header),
                Some(Typed::Styles) => write_styles(&mut output, header),
                Some(Typed::Events) => write_events(&mut output, subtitles, warnings),
                None => {}
            }
        }
        position += 1;
    }
    while output.ends_with("\n\n") {
        output.pop();
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn write_raw_section(output: &mut String, section: &RawSection) {
    output.push('[');
    output.push_str(&section.name);
    output.push_str("]\n");
    let last_nonempty = section
        .lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(0, |position| position + 1);
    for line in &section.lines[..last_nonempty] {
        output.push_str(line);
        output.push('\n');
    }
}

fn write_script_info(output: &mut String, header: &Header) {
    output.push_str("[Script Info]\n");
    if header.script_info.is_empty() {
        output.push_str("ScriptType: v4.00+\n");
        return;
    }
    for (key, value) in &header.script_info {
        if key.trim_start().starts_with(';') && value.is_empty() {
            output.push_str(key);
        } else {
            output.push_str(key);
            output.push_str(": ");
            output.push_str(value);
        }
        output.push('\n');
    }
}

fn write_styles(output: &mut String, header: &Header) {
    output.push_str("[V4+ Styles]\n");
    output.push_str(STYLE_FORMAT_LINE);
    output.push('\n');
    if header.styles.is_empty() {
        write_style(output, &Style::default());
    } else {
        for style in &header.styles {
            write_style(output, style);
        }
    }
}

fn write_style(output: &mut String, style: &Style) {
    let values = [
        style.name.clone(),
        style.font_name.clone(),
        style.font_size.to_string(),
        style.primary_colour.to_string(),
        style.secondary_colour.to_string(),
        style.outline_colour.to_string(),
        style.back_colour.to_string(),
        bool_number(style.bold).to_owned(),
        bool_number(style.italic).to_owned(),
        bool_number(style.underline).to_owned(),
        bool_number(style.strike_out).to_owned(),
        style.scale_x.to_string(),
        style.scale_y.to_string(),
        style.spacing.to_string(),
        style.angle.to_string(),
        style.border_style.to_string(),
        style.outline.to_string(),
        style.shadow.to_string(),
        style.alignment.to_string(),
        style.margin_l.to_string(),
        style.margin_r.to_string(),
        style.margin_v.to_string(),
        style.encoding.to_string(),
    ];
    output.push_str("Style: ");
    output.push_str(&values.join(","));
    output.push('\n');
}

/// Non-dialogue events are written at their recorded row. Events whose row
/// lies past the last cue, which happens after cues were removed, follow
/// the last cue in row order.
fn write_events(output: &mut String, subtitles: &Subtitles, warnings: &mut Vec<Warning>) {
    output.push_str("[Events]\n");
    output.push_str(EVENT_FORMAT_LINE);
    output.push('\n');
    let already_warned: HashSet<usize> = warnings
        .iter()
        .filter_map(|warning| match warning {
            Warning::TimePrecisionLost { cue } => Some(*cue),
            _ => None,
        })
        .collect();
    let mut events: Vec<&AssEvent> = subtitles.events.iter().collect();
    events.sort_by_key(|event| event.event_index);
    let mut next_event = 0;
    for (cue_index, cue) in subtitles.cues.iter().enumerate() {
        while events
            .get(next_event)
            .is_some_and(|event| event.event_index <= cue_index + next_event)
        {
            write_event(output, events[next_event]);
            next_event += 1;
        }
        let cue_number = cue_index + 1;
        if (cue.start.0 % 10 != 0 || cue.end.0 % 10 != 0) && !already_warned.contains(&cue_number) {
            warnings.push(Warning::TimePrecisionLost { cue: cue_number });
        }
        let fields = cue.ass.clone().unwrap_or_default();
        write_ass_row(
            output,
            EventKind::Dialogue,
            &fields,
            cue.start,
            cue.end,
            &cue.text,
        );
    }
    for event in &events[next_event..] {
        write_event(output, event);
    }
}

fn write_event(output: &mut String, event: &AssEvent) {
    let fields = AssFields {
        kind: event.kind,
        layer: event.layer,
        marked: event.marked,
        style: event.style.clone(),
        name: event.name.clone(),
        margin_l: event.margin_l,
        margin_r: event.margin_r,
        margin_v: event.margin_v,
        effect: event.effect.clone(),
    };
    write_ass_row(
        output,
        event.kind,
        &fields,
        event.start,
        event.end,
        &event.text,
    );
}

fn write_ass_row(
    output: &mut String,
    kind: EventKind,
    fields: &AssFields,
    start: Time,
    end: Time,
    text: &str,
) {
    output.push_str(kind.as_str());
    output.push_str(": ");
    output.push_str(&fields.layer.to_string());
    output.push(',');
    output.push_str(&format_ass_time(start));
    output.push(',');
    output.push_str(&format_ass_time(end));
    output.push(',');
    output.push_str(&fields.style);
    output.push(',');
    output.push_str(&fields.name);
    output.push(',');
    output.push_str(&fields.margin_l.to_string());
    output.push(',');
    output.push_str(&fields.margin_r.to_string());
    output.push(',');
    output.push_str(&fields.margin_v.to_string());
    output.push(',');
    output.push_str(&fields.effect);
    output.push(',');
    for character in text.chars() {
        if character == '\n' {
            output.push_str("\\N");
        } else {
            output.push(character);
        }
    }
    output.push('\n');
}

fn bool_number(value: bool) -> &'static str {
    if value {
        "-1"
    } else {
        "0"
    }
}

fn split_time(time: Time) -> (i64, i64, i64, i64) {
    let millis = time.0.max(0);
    (
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1_000 % 60,
        millis % 1_000,
    )
}

fn format_srt_time(time: Time) -> String {
    let (hours, minutes, seconds, millis) = split_time(time);
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

fn format_vtt_time(time: Time) -> String {
    let (hours, minutes, seconds, millis) = split_time(time);
    format!("{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
}

fn format_ass_time(time: Time) -> String {
    let (hours, minutes, seconds, millis) = split_time(time);
    format!("{hours}:{minutes:02}:{seconds:02}.{:02}", millis / 10)
}

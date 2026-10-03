use std::collections::HashSet;

use crate::model::{
    AssEvent, AssFields, AssNumber, Colour, Cue, Error, EventKind, Format, Header, RawSection,
    Style, Subtitles, Time, VttBlock, VttBlockKind, Warning,
};

const EVENT_FORMAT: [&str; 10] = [
    "Layer", "Start", "End", "Style", "Name", "MarginL", "MarginR", "MarginV", "Effect", "Text",
];

const STYLE_FORMAT: [&str; 23] = [
    "Name",
    "Fontname",
    "Fontsize",
    "PrimaryColour",
    "SecondaryColour",
    "OutlineColour",
    "BackColour",
    "Bold",
    "Italic",
    "Underline",
    "StrikeOut",
    "ScaleX",
    "ScaleY",
    "Spacing",
    "Angle",
    "BorderStyle",
    "Outline",
    "Shadow",
    "Alignment",
    "MarginL",
    "MarginR",
    "MarginV",
    "Encoding",
];

const V4_STYLE_FORMAT: [&str; 18] = [
    "Name",
    "Fontname",
    "Fontsize",
    "PrimaryColour",
    "SecondaryColour",
    "TertiaryColour",
    "BackColour",
    "Bold",
    "Italic",
    "BorderStyle",
    "Outline",
    "Shadow",
    "Alignment",
    "MarginL",
    "MarginR",
    "MarginV",
    "AlphaLevel",
    "Encoding",
];

#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    number: usize,
}

struct Decoded<'a> {
    lines: Vec<Line<'a>>,
    warnings: Vec<Warning>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TimeFlags {
    component_out_of_range: bool,
    fraction_digits: usize,
    separator: u8,
    omitted_hours: bool,
    hour_digits: usize,
}

fn decode(input: &[u8]) -> Result<Decoded<'_>, Error> {
    if input.starts_with(&[0xff, 0xfe]) || input.starts_with(&[0xfe, 0xff]) {
        return Err(Error::Utf16Input);
    }
    let (bytes, had_bom) = if input.starts_with(&[0xef, 0xbb, 0xbf]) {
        (&input[3..], true)
    } else {
        (input, false)
    };
    let text = std::str::from_utf8(bytes).map_err(|error| Error::InvalidUtf8 {
        offset: error.valid_up_to() + usize::from(had_bom) * 3,
    })?;
    let mut warnings = Vec::new();
    if had_bom {
        warnings.push(Warning::BomStripped);
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut number = 1;
    let bytes = text.as_bytes();
    let mut endings = 0_u8;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                endings |= 1;
                lines.push(Line {
                    text: &text[start..index],
                    number,
                });
                number += 1;
                index += 1;
                start = index;
            }
            b'\r' => {
                lines.push(Line {
                    text: &text[start..index],
                    number,
                });
                number += 1;
                if bytes.get(index + 1) == Some(&b'\n') {
                    endings |= 2;
                    index += 2;
                } else {
                    endings |= 4;
                    index += 1;
                }
                start = index;
            }
            _ => index += 1,
        }
    }
    if start < bytes.len() {
        lines.push(Line {
            text: &text[start..],
            number,
        });
    }
    if endings.count_ones() > 1 {
        warnings.push(Warning::MixedLineEndings);
    }
    Ok(Decoded { lines, warnings })
}

fn parse_unsigned(input: &str, line: usize) -> Result<i64, Error> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidTimestamp { line });
    }
    input
        .parse::<i64>()
        .map_err(|_| Error::TimeOutOfRange { line })
}

pub(crate) fn parse_time(
    input: &str,
    line: usize,
    allowed_separators: &[u8],
    allow_omitted_hours: bool,
) -> Result<(Time, TimeFlags), Error> {
    let input = input.trim();
    if input.starts_with('-') {
        return Err(Error::NegativeTime { line });
    }
    let separator_index = input
        .bytes()
        .rposition(|byte| allowed_separators.contains(&byte))
        .ok_or(Error::InvalidTimestamp { line })?;
    let separator = input.as_bytes()[separator_index];
    let clock = &input[..separator_index];
    let fraction = &input[separator_index + 1..];
    if !(1..=3).contains(&fraction.len()) || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidTimestamp { line });
    }
    let mut parts = clock.split(':');
    let first = parts.next().ok_or(Error::InvalidTimestamp { line })?;
    let second = parts.next().ok_or(Error::InvalidTimestamp { line })?;
    let third = parts.next();
    let fourth = parts.next();
    let (hours_text, minutes_text, seconds_text, omitted_hours) = match (third, fourth) {
        (None, None) if allow_omitted_hours => ("0", first, second, true),
        (Some(seconds), None) => (first, second, seconds, false),
        _ => return Err(Error::InvalidTimestamp { line }),
    };
    if minutes_text.len() > 2 || seconds_text.len() > 2 {
        return Err(Error::InvalidTimestamp { line });
    }
    let hours = parse_unsigned(hours_text, line)?;
    let minutes = parse_unsigned(minutes_text, line)?;
    let seconds = parse_unsigned(seconds_text, line)?;
    let fraction_value = parse_unsigned(fraction, line)?;
    let fraction_ms = match fraction.len() {
        1 => fraction_value.checked_mul(100),
        2 => fraction_value.checked_mul(10),
        3 => Some(fraction_value),
        _ => None,
    }
    .ok_or(Error::TimeOutOfRange { line })?;
    let millis = hours
        .checked_mul(60)
        .and_then(|value| value.checked_add(minutes))
        .and_then(|value| value.checked_mul(60))
        .and_then(|value| value.checked_add(seconds))
        .and_then(|value| value.checked_mul(1000))
        .and_then(|value| value.checked_add(fraction_ms))
        .ok_or(Error::TimeOutOfRange { line })?;
    Ok((
        Time(millis),
        TimeFlags {
            component_out_of_range: minutes > 59 || seconds > 59,
            fraction_digits: fraction.len(),
            separator,
            omitted_hours,
            hour_digits: hours_text.len(),
        },
    ))
}

pub(crate) fn split_timing(line: &str, number: usize) -> Result<(&str, &str, &str), Error> {
    let (start, rest) = line
        .split_once("-->")
        .ok_or(Error::InvalidTimestamp { line: number })?;
    let rest = rest.trim_start();
    let boundary = rest
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index));
    let (end, suffix) = match boundary {
        Some(index) => (&rest[..index], rest[index..].trim()),
        None => (rest, ""),
    };
    if start.trim().is_empty() || end.is_empty() {
        return Err(Error::InvalidTimestamp { line: number });
    }
    Ok((start.trim(), end, suffix))
}

pub(crate) fn push_time_warnings(
    warnings: &mut Vec<Warning>,
    line: usize,
    first: TimeFlags,
    second: TimeFlags,
    format: Format,
) {
    if first.component_out_of_range || second.component_out_of_range {
        warnings.push(Warning::ComponentOutOfRange { line });
    }
    match format {
        Format::Srt => {
            if first.separator == b'.' || second.separator == b'.' {
                warnings.push(Warning::DotMillis { line });
            }
            if first.fraction_digits < 3 || second.fraction_digits < 3 {
                warnings.push(Warning::ShortMillis { line });
            }
            if first.omitted_hours || second.omitted_hours {
                warnings.push(Warning::MissingHours { line });
            }
        }
        Format::Vtt => {
            if first.separator == b',' || second.separator == b',' {
                warnings.push(Warning::CommaMillis { line });
            }
            if (!first.omitted_hours && first.hour_digits == 1)
                || (!second.omitted_hours && second.hour_digits == 1)
            {
                warnings.push(Warning::ShortHours { line });
            }
        }
        Format::Ass => {
            if first.fraction_digits != 2 || second.fraction_digits != 2 {
                warnings.push(Warning::AssFractionDigits { line });
            }
        }
    }
}

pub(crate) fn parse_srt(input: &[u8]) -> Result<Subtitles, Error> {
    let decoded = decode(input)?;
    let lines = decoded.lines;
    let mut warnings = decoded.warnings;
    let mut timing_positions: Vec<usize> = Vec::new();
    for (position, line) in lines.iter().enumerate() {
        let previous = position.checked_sub(1).map(|index| lines[index].text);
        let block_position = previous.map_or(true, is_blank_or_digits);
        if is_timing_line(line.text, block_position) {
            timing_positions.push(position);
        } else if timing_positions.is_empty()
            && looks_like_time(line.text)
            && previous.map_or(true, is_blank_or_digits)
        {
            return Err(Error::MissingArrow { line: line.number });
        }
    }
    if timing_positions.is_empty() {
        warnings.push(Warning::NoCues);
        return Ok(Subtitles {
            format: Format::Srt,
            header: Header::default(),
            cues: Vec::new(),
            events: Vec::new(),
            warnings,
        });
    }

    let first_timing = timing_positions[0];
    let first_block_start = srt_block_start(&lines, first_timing);
    for line in &lines[..first_block_start] {
        let trimmed = line.text.trim();
        if !trimmed.is_empty() && !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
            warnings.push(Warning::StrayText { line: line.number });
        }
    }

    let mut cues = Vec::with_capacity(timing_positions.len());
    let mut expected_index = None;
    for (cue_index, &timing_position) in timing_positions.iter().enumerate() {
        let timing_line = lines[timing_position];
        let block_start = srt_block_start(&lines, timing_position);
        let index = if block_start < timing_position {
            let index_line = lines[block_start];
            let parsed = index_line.text.trim().parse::<u32>().ok();
            match parsed {
                Some(found) => {
                    if expected_index.is_some_and(|expected| found != expected) {
                        warnings.push(Warning::NonSequentialIndex {
                            line: index_line.number,
                            expected: expected_index.unwrap_or(found),
                            found,
                        });
                    }
                    expected_index = found.checked_add(1);
                    Some(found)
                }
                None => {
                    warnings.push(Warning::MissingIndex {
                        line: timing_line.number,
                    });
                    None
                }
            }
        } else {
            warnings.push(Warning::MissingIndex {
                line: timing_line.number,
            });
            None
        };
        let (start_text, end_text, suffix) = split_timing(timing_line.text, timing_line.number)?;
        let (start, start_flags) = parse_time(start_text, timing_line.number, b",.", true)?;
        let (end, end_flags) = parse_time(end_text, timing_line.number, b",.", true)?;
        push_time_warnings(
            &mut warnings,
            timing_line.number,
            start_flags,
            end_flags,
            Format::Srt,
        );
        let settings = if suffix.is_empty() {
            None
        } else {
            if !is_position_suffix(suffix) {
                warnings.push(Warning::UnknownTimingSuffix {
                    line: timing_line.number,
                });
            }
            Some(suffix.to_owned())
        };
        let next_block_start = timing_positions
            .get(cue_index + 1)
            .map_or(lines.len(), |position| srt_block_start(&lines, *position));
        let text_lines = &lines[timing_position + 1..next_block_start];
        let text = parse_srt_text(text_lines, cue_index + 1, &mut warnings);
        if next_block_start < lines.len()
            && !text_lines.iter().any(|line| line.text.is_empty())
            && text_lines.last().is_some_and(|line| !line.text.is_empty())
        {
            warnings.push(Warning::MissingBlankLine {
                line: lines[next_block_start].number,
            });
        }
        if end < start {
            warnings.push(Warning::EndBeforeStart { cue: cue_index + 1 });
        }
        cues.push(Cue {
            index,
            id: None,
            start,
            end,
            text,
            settings,
            ass: None,
        });
    }
    Ok(Subtitles {
        format: Format::Srt,
        header: Header::default(),
        cues,
        events: Vec::new(),
        warnings,
    })
}

fn is_blank_or_digits(line: &str) -> bool {
    line.trim().bytes().all(|byte| byte.is_ascii_digit())
}

/// A line holding `-->` is a timing line when it sits in block position
/// (start of input, or after a blank or index line). Anywhere else it is a
/// timing line only when both times parse, so `go --> there` inside cue
/// text stays text while a cue with no text can still be followed directly
/// by the next timing line.
pub(crate) fn is_timing_line(text: &str, block_position: bool) -> bool {
    text.contains("-->") && (block_position || timing_parses(text))
}

pub(crate) fn timing_parses(text: &str) -> bool {
    split_timing(text, 0).is_ok_and(|(start, end, _)| {
        parse_time(start, 0, b",.", true).is_ok() && parse_time(end, 0, b",.", true).is_ok()
    })
}

/// A line reads as a time when its first token is a timestamp, or when it
/// holds a single-dash arrow between two timestamps.
pub(crate) fn looks_like_time(text: &str) -> bool {
    let trimmed = text.trim();
    if let Some((start, end)) = trimmed.split_once("->") {
        return parse_time(start, 0, b",.", true).is_ok()
            && parse_time(end, 0, b",.", true).is_ok();
    }
    trimmed
        .split_whitespace()
        .next()
        .is_some_and(|token| parse_time(token, 0, b",.", true).is_ok())
}

fn srt_block_start(lines: &[Line<'_>], timing_position: usize) -> usize {
    timing_position
        .checked_sub(1)
        .filter(|position| {
            let trimmed = lines[*position].text.trim();
            !trimmed.is_empty() && trimmed.bytes().all(|byte| byte.is_ascii_digit())
        })
        .unwrap_or(timing_position)
}

fn parse_srt_text(lines: &[Line<'_>], cue: usize, warnings: &mut Vec<Warning>) -> String {
    if lines.first().is_some_and(|line| line.text.is_empty()) {
        warnings.push(Warning::EmptyCue { cue });
        if let Some(stray) = lines.iter().skip(1).find(|line| !line.text.is_empty()) {
            warnings.push(Warning::StrayText { line: stray.number });
        }
        return String::new();
    }
    let first_blank = lines.iter().position(|line| line.text.is_empty());
    let mut selected = match first_blank {
        Some(blank) => &lines[..blank],
        None => lines,
    };
    let mut digit_tail = Vec::new();
    if let Some(blank) = first_blank {
        for line in &lines[blank + 1..] {
            let trimmed = line.text.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
                warnings.push(Warning::StrayText { line: line.number });
                digit_tail.push(line.text);
            } else {
                warnings.push(Warning::StrayText { line: line.number });
            }
        }
    }
    while selected.last().is_some_and(|line| line.text.is_empty()) {
        selected = &selected[..selected.len() - 1];
    }
    let mut text = selected
        .iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");
    for tail in digit_tail {
        text.push_str("\n\n");
        text.push_str(tail);
    }
    if text.is_empty() {
        warnings.push(Warning::EmptyCue { cue });
    }
    text
}

pub(crate) fn is_position_suffix(suffix: &str) -> bool {
    let mut parts = suffix.split_ascii_whitespace();
    ["X1:", "X2:", "Y1:", "Y2:"].iter().all(|prefix| {
        parts.next().is_some_and(|part| {
            part.strip_prefix(prefix).is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
    }) && parts.next().is_none()
}

pub(crate) fn parse_vtt(input: &[u8]) -> Result<Subtitles, Error> {
    let decoded = decode(input)?;
    let lines = decoded.lines;
    let mut warnings = decoded.warnings;
    let first = lines.first().ok_or_else(|| Error::MissingHeader {
        line: 1,
        found: String::new(),
    })?;
    let signature = first
        .text
        .strip_prefix("WEBVTT")
        .filter(|suffix| suffix.is_empty() || suffix.starts_with(' ') || suffix.starts_with('\t'))
        .ok_or_else(|| Error::MissingHeader {
            line: 1,
            found: first.text.to_owned(),
        })?;
    let mut header = Header {
        vtt_signature: (!signature.trim().is_empty()).then(|| signature.trim().to_owned()),
        ..Header::default()
    };
    let mut position = 1;
    let mut missing_header_blank = false;
    while position < lines.len() && !lines[position].text.is_empty() {
        if lines[position].text.contains("-->") {
            warnings.push(Warning::MissingBlankLine {
                line: lines[position].number,
            });
            missing_header_blank = true;
            break;
        }
        header
            .vtt_header_lines
            .push(lines[position].text.to_owned());
        position += 1;
    }
    if !missing_header_blank && position < lines.len() && lines[position].text.is_empty() {
        position += 1;
    }
    let mut cues = Vec::new();
    while position < lines.len() {
        while position < lines.len() && lines[position].text.is_empty() {
            position += 1;
        }
        if position >= lines.len() {
            break;
        }
        let start = position;
        while position < lines.len() && !lines[position].text.is_empty() {
            position += 1;
        }
        let block = &lines[start..position];
        let first_text = block[0].text;
        let kind = if first_text == "REGION" {
            Some(VttBlockKind::Region)
        } else if first_text == "STYLE" {
            Some(VttBlockKind::Style)
        } else if first_text == "NOTE"
            || first_text.starts_with("NOTE ")
            || first_text.starts_with("NOTE\t")
        {
            Some(VttBlockKind::Note)
        } else {
            None
        };
        if let Some(kind) = kind {
            header.blocks.push(VttBlock {
                kind,
                raw: block
                    .iter()
                    .map(|line| line.text)
                    .collect::<Vec<_>>()
                    .join("\n"),
                cue_index: cues.len(),
            });
            continue;
        }
        let timing_offset = if block[0].text.contains("-->") { 0 } else { 1 };
        let Some(timing_line) = block
            .get(timing_offset)
            .filter(|line| line.text.contains("-->"))
        else {
            warnings.push(Warning::StrayText {
                line: block[0].number,
            });
            continue;
        };
        let mut id = (timing_offset == 1).then(|| block[0].text.to_owned());
        let mut timing_line = *timing_line;
        let mut text_start = timing_offset + 1;
        loop {
            let text_end = block[text_start..]
                .iter()
                .position(|line| line.text.contains("-->") && timing_parses(line.text))
                .map_or(block.len(), |offset| text_start + offset);
            let (start_text, end_text, suffix) =
                split_timing(timing_line.text, timing_line.number)?;
            let (cue_start, first_flags) = parse_time(start_text, timing_line.number, b".,", true)?;
            let (cue_end, second_flags) = parse_time(end_text, timing_line.number, b".,", true)?;
            push_time_warnings(
                &mut warnings,
                timing_line.number,
                first_flags,
                second_flags,
                Format::Vtt,
            );
            let cue_number = cues.len() + 1;
            if cue_end < cue_start {
                warnings.push(Warning::EndBeforeStart { cue: cue_number });
            }
            cues.push(Cue {
                index: None,
                id: id.take(),
                start: cue_start,
                end: cue_end,
                text: block[text_start..text_end]
                    .iter()
                    .map(|line| line.text)
                    .collect::<Vec<_>>()
                    .join("\n"),
                settings: (!suffix.is_empty()).then(|| suffix.to_owned()),
                ass: None,
            });
            if text_end == block.len() {
                break;
            }
            timing_line = block[text_end];
            warnings.push(Warning::MissingBlankLine {
                line: timing_line.number,
            });
            text_start = text_end + 1;
        }
    }
    if cues.is_empty() {
        warnings.push(Warning::NoCues);
    }
    Ok(Subtitles {
        format: Format::Vtt,
        header,
        cues,
        events: Vec::new(),
        warnings,
    })
}

pub(crate) fn parse_ass(input: &[u8]) -> Result<Subtitles, Error> {
    let decoded = decode(input)?;
    let lines = decoded.lines;
    let mut warnings = decoded.warnings;
    if lines.iter().all(|line| line.text.trim().is_empty()) {
        warnings.push(Warning::NoCues);
        return Ok(Subtitles {
            format: Format::Ass,
            header: Header::default(),
            cues: Vec::new(),
            events: Vec::new(),
            warnings,
        });
    }
    let mut sections = Vec::new();
    let mut current: Option<(String, usize, usize)> = None;
    for (position, line) in lines.iter().enumerate() {
        let trimmed = line.text.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
            if let Some((name, header_position, body_start)) = current.take() {
                sections.push((name, header_position, body_start, position));
            }
            current = Some((
                trimmed[1..trimmed.len() - 1].trim().to_owned(),
                position,
                position + 1,
            ));
        } else if current.is_none() && !trimmed.is_empty() && !trimmed.starts_with(';') {
            return Err(Error::NotAss { line: line.number });
        }
    }
    if let Some((name, header_position, body_start)) = current {
        sections.push((name, header_position, body_start, lines.len()));
    }
    if sections.is_empty() {
        warnings.push(Warning::NoCues);
        return Ok(Subtitles {
            format: Format::Ass,
            header: Header::default(),
            cues: Vec::new(),
            events: Vec::new(),
            warnings,
        });
    }

    let mut header = Header::default();
    let mut cues = Vec::new();
    let mut events = Vec::new();
    let mut saw_styles = false;
    let mut upgraded = false;
    let mut event_counter = 0;
    for (section_position, (name, header_position, body_start, body_end)) in
        sections.iter().enumerate()
    {
        let lower = name.to_ascii_lowercase();
        let body = &lines[*body_start..*body_end];
        match lower.as_str() {
            "script info" => parse_script_info(body, &mut header),
            "v4+ styles" | "v4 styles" => {
                saw_styles = true;
                let is_v4 = lower == "v4 styles";
                upgraded |= is_v4;
                parse_styles(body, is_v4, &mut header.styles, &mut warnings);
            }
            "events" => {
                parse_events(
                    body,
                    &mut cues,
                    &mut events,
                    &mut warnings,
                    &mut event_counter,
                )?;
            }
            _ => {
                if !is_known_section(&lower) {
                    warnings.push(Warning::UnknownSection {
                        line: lines[*header_position].number,
                        name: name.clone(),
                    });
                }
                header.raw_sections.push(RawSection {
                    name: name.clone(),
                    lines: body.iter().map(|line| line.text.to_owned()).collect(),
                    position: section_position,
                });
            }
        }
    }
    if upgraded {
        warnings.push(Warning::UpgradedToV4Plus);
        for (key, value) in &mut header.script_info {
            if key.eq_ignore_ascii_case("ScriptType") {
                *value = "v4.00+".to_owned();
            }
        }
    }
    if !saw_styles
        && header.raw_sections.iter().any(|section| {
            section
                .lines
                .iter()
                .any(|line| strip_key(line.trim_start(), "Style").is_some())
        })
    {
        saw_styles = true;
    }
    if !saw_styles {
        warnings.push(Warning::MissingStyles);
    }
    if cues.is_empty() {
        warnings.push(Warning::NoCues);
    }
    let style_names = style_name_set(&header.styles);
    if saw_styles && !header.styles.is_empty() {
        for (index, cue) in cues.iter().enumerate() {
            if let Some(fields) = &cue.ass {
                if !style_names.contains(&fields.style.to_ascii_lowercase()) {
                    warnings.push(Warning::UnknownStyle {
                        cue: index + 1,
                        style: fields.style.clone(),
                    });
                }
            }
        }
    }
    Ok(Subtitles {
        format: Format::Ass,
        header,
        cues,
        events,
        warnings,
    })
}

pub(crate) fn style_name_set(styles: &[Style]) -> HashSet<String> {
    styles
        .iter()
        .map(|style| style.name.to_ascii_lowercase())
        .collect()
}

fn parse_script_info(lines: &[Line<'_>], header: &mut Header) {
    for line in lines {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with(';') {
            header
                .script_info
                .push((line.text.to_owned(), String::new()));
        } else if let Some((key, value)) = line.text.split_once(':') {
            header
                .script_info
                .push((key.trim().to_owned(), value.trim_start().to_owned()));
        }
    }
}

fn parse_styles(
    lines: &[Line<'_>],
    is_v4: bool,
    styles: &mut Vec<Style>,
    warnings: &mut Vec<Warning>,
) {
    let mut format: Option<Vec<String>> = None;
    for line in lines {
        let trimmed = line.text.trim_start();
        if let Some(value) = strip_key(trimmed, "Format") {
            format = Some(
                value
                    .split(',')
                    .map(|field| field.trim().to_owned())
                    .collect(),
            );
        } else if let Some(value) = strip_key(trimmed, "Style") {
            styles.push(parse_style_line(
                value,
                format.as_deref(),
                is_v4,
                line.number,
                warnings,
            ));
        }
    }
}

pub(crate) fn parse_style_line(
    value: &str,
    format: Option<&[String]>,
    is_v4: bool,
    line: usize,
    warnings: &mut Vec<Warning>,
) -> Style {
    let fallback: Vec<String> = if is_v4 {
        V4_STYLE_FORMAT
            .iter()
            .map(|field| (*field).to_owned())
            .collect()
    } else {
        STYLE_FORMAT
            .iter()
            .map(|field| (*field).to_owned())
            .collect()
    };
    parse_style(value, format.unwrap_or(&fallback), is_v4, line, warnings)
}

pub(crate) fn parse_style_format(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|field| field.trim().to_owned())
        .collect()
}

fn parse_style(
    value: &str,
    format: &[String],
    is_v4: bool,
    line: usize,
    warnings: &mut Vec<Warning>,
) -> Style {
    let values: Vec<&str> = value.split(',').collect();
    if values.len() != format.len() {
        warnings.push(Warning::FieldCountMismatch {
            line,
            expected: format.len(),
            found: values.len(),
        });
    }
    let mut style = Style::default();
    for (index, name) in format.iter().enumerate() {
        let Some(value) = values.get(index).copied() else {
            continue;
        };
        let value = value.trim();
        let result = set_style_field(&mut style, name, value, is_v4);
        if result.is_err() {
            warnings.push(Warning::BadField {
                line,
                field: name.clone(),
            });
        }
    }
    style
}

fn set_style_field(style: &mut Style, name: &str, value: &str, is_v4: bool) -> Result<(), ()> {
    match name.to_ascii_lowercase().as_str() {
        "name" => style.name = value.to_owned(),
        "fontname" => style.font_name = value.to_owned(),
        "fontsize" => style.font_size = AssNumber::parse(value).ok_or(())?,
        "primarycolour" => style.primary_colour = parse_colour(value).ok_or(())?,
        "secondarycolour" => style.secondary_colour = parse_colour(value).ok_or(())?,
        "outlinecolour" | "tertiarycolour" => {
            style.outline_colour = parse_colour(value).ok_or(())?
        }
        "backcolour" => style.back_colour = parse_colour(value).ok_or(())?,
        "bold" => style.bold = parse_ass_bool(value).ok_or(())?,
        "italic" => style.italic = parse_ass_bool(value).ok_or(())?,
        "underline" => style.underline = parse_ass_bool(value).ok_or(())?,
        "strikeout" => style.strike_out = parse_ass_bool(value).ok_or(())?,
        "scalex" => style.scale_x = AssNumber::parse(value).ok_or(())?,
        "scaley" => style.scale_y = AssNumber::parse(value).ok_or(())?,
        "spacing" => style.spacing = AssNumber::parse(value).ok_or(())?,
        "angle" => style.angle = AssNumber::parse(value).ok_or(())?,
        "borderstyle" => style.border_style = value.parse().map_err(|_| ())?,
        "outline" => style.outline = AssNumber::parse(value).ok_or(())?,
        "shadow" => style.shadow = AssNumber::parse(value).ok_or(())?,
        "alignment" => style.alignment = value.parse().map_err(|_| ())?,
        "marginl" => style.margin_l = value.parse().map_err(|_| ())?,
        "marginr" => style.margin_r = value.parse().map_err(|_| ())?,
        "marginv" => style.margin_v = value.parse().map_err(|_| ())?,
        "encoding" => style.encoding = value.parse().map_err(|_| ())?,
        "alphalevel" if is_v4 => {}
        _ => {}
    }
    Ok(())
}

fn parse_colour(value: &str) -> Option<Colour> {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("&H")
        .or_else(|| value.strip_prefix("&h"))
    {
        let hex = hex.strip_suffix('&').unwrap_or(hex);
        u32::from_str_radix(hex, 16).ok().map(Colour)
    } else {
        value
            .parse::<i64>()
            .ok()
            .map(|number| Colour(number as u32))
    }
}

fn parse_ass_bool(value: &str) -> Option<bool> {
    match value {
        "-1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

fn parse_events(
    lines: &[Line<'_>],
    cues: &mut Vec<Cue>,
    events: &mut Vec<AssEvent>,
    warnings: &mut Vec<Warning>,
    event_counter: &mut usize,
) -> Result<(), Error> {
    let mut format: Option<Vec<String>> = None;
    let mut warned_missing = false;
    let fallback: Vec<String> = EVENT_FORMAT
        .iter()
        .map(|field| (*field).to_owned())
        .collect();
    for line in lines {
        let trimmed = line.text.trim_start();
        if let Some(value) = strip_key(trimmed, "Format") {
            format = Some(parse_event_format(value, warnings));
            continue;
        }
        let Some((kind, value)) = parse_event_prefix(trimmed) else {
            continue;
        };
        let fields = if let Some(format) = &format {
            format
        } else {
            if !warned_missing {
                warnings.push(Warning::MissingFormatLine {
                    section: "Events".to_owned(),
                });
                warned_missing = true;
            }
            &fallback
        };
        let values: Vec<&str> = value.splitn(fields.len(), ',').collect();
        if values.len() != fields.len() {
            warnings.push(Warning::FieldCountMismatch {
                line: line.number,
                expected: fields.len(),
                found: values.len(),
            });
        }
        let get = |field: &str| -> &str {
            fields
                .iter()
                .position(|name| name.eq_ignore_ascii_case(field))
                .and_then(|position| values.get(position).copied())
                .unwrap_or("")
        };
        let start_text = get("Start").trim();
        let end_text = get("End").trim();
        let timing_line = line.number;
        let (start, first_flags) = parse_time(start_text, timing_line, b".", false)?;
        let (end, second_flags) = parse_time(end_text, timing_line, b".", false)?;
        push_time_warnings(
            warnings,
            timing_line,
            first_flags,
            second_flags,
            Format::Ass,
        );
        let marked_text = get("Marked").trim();
        let marked = marked_text.strip_prefix("Marked=").unwrap_or(marked_text) == "1";
        let layer = parse_i32_field(get("Layer"), "Layer", line.number, warnings);
        let margin_l = parse_i32_field(get("MarginL"), "MarginL", line.number, warnings);
        let margin_r = parse_i32_field(get("MarginR"), "MarginR", line.number, warnings);
        let margin_v = parse_i32_field(get("MarginV"), "MarginV", line.number, warnings);
        let fields = AssFields {
            kind,
            layer,
            marked,
            style: get("Style").trim().to_owned(),
            name: get("Name").trim().to_owned(),
            margin_l,
            margin_r,
            margin_v,
            effect: get("Effect").trim().to_owned(),
        };
        let text = get("Text").trim_start().to_owned();
        if end < start {
            warnings.push(Warning::EndBeforeStart {
                cue: cues.len() + 1,
            });
        }
        if kind == EventKind::Dialogue {
            cues.push(Cue {
                index: None,
                id: None,
                start,
                end,
                text,
                settings: None,
                ass: Some(fields),
            });
        } else {
            events.push(AssEvent {
                kind,
                layer: fields.layer,
                marked: fields.marked,
                start,
                end,
                style: fields.style,
                name: fields.name,
                margin_l: fields.margin_l,
                margin_r: fields.margin_r,
                margin_v: fields.margin_v,
                effect: fields.effect,
                text,
                event_index: *event_counter,
            });
        }
        *event_counter += 1;
    }
    Ok(())
}

pub(crate) fn parse_event_format(value: &str, warnings: &mut Vec<Warning>) -> Vec<String> {
    let parsed: Vec<String> = value
        .split(',')
        .map(|field| field.trim().to_owned())
        .collect();
    let lower: Vec<String> = parsed
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect();
    let canonical: Vec<String> = EVENT_FORMAT
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect();
    let mut v4 = canonical.clone();
    v4[0] = "marked".to_owned();
    if lower != canonical && lower != v4 {
        warnings.push(Warning::FormatReordered);
    }
    parsed
}

pub(crate) fn is_known_section(lower: &str) -> bool {
    matches!(
        lower,
        "script info"
            | "v4+ styles"
            | "v4 styles"
            | "events"
            | "fonts"
            | "graphics"
            | "aegisub project garbage"
            | "aegisub extradata"
    )
}

fn parse_i32_field(value: &str, field: &str, line: usize, warnings: &mut Vec<Warning>) -> i32 {
    if value.trim().is_empty() {
        return 0;
    }
    value.trim().parse().unwrap_or_else(|_| {
        warnings.push(Warning::BadField {
            line,
            field: field.to_owned(),
        });
        0
    })
}

fn strip_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (found, value) = line.split_once(':')?;
    found
        .trim()
        .eq_ignore_ascii_case(key)
        .then(|| value.trim_start())
}

fn parse_event_prefix(line: &str) -> Option<(EventKind, &str)> {
    [
        ("Dialogue", EventKind::Dialogue),
        ("Comment", EventKind::Comment),
        ("Picture", EventKind::Picture),
        ("Sound", EventKind::Sound),
        ("Movie", EventKind::Movie),
        ("Command", EventKind::Command),
    ]
    .into_iter()
    .find_map(|(name, kind)| strip_key(line, name).map(|value| (kind, value)))
}

pub(crate) fn parse_ass_stream_event(
    line: &str,
    line_number: usize,
    format: Option<&[String]>,
    cue_number: usize,
) -> Result<(Option<Cue>, Vec<Warning>), Error> {
    let mut warnings = Vec::new();
    let trimmed = line.trim_start();
    let Some((kind, value)) = parse_event_prefix(trimmed) else {
        return Ok((None, warnings));
    };
    let field_count = format.map_or(EVENT_FORMAT.len(), <[String]>::len);
    let mut values = Vec::with_capacity(field_count);
    values.extend(value.splitn(field_count, ','));
    if values.len() != field_count {
        warnings.push(Warning::FieldCountMismatch {
            line: line_number,
            expected: field_count,
            found: values.len(),
        });
    }
    let get = |field: &str| -> &str {
        let position = format.map_or_else(
            || {
                EVENT_FORMAT
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(field))
            },
            |fields| {
                fields
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(field))
            },
        );
        position
            .and_then(|position| values.get(position).copied())
            .unwrap_or("")
    };
    let timing_line = line_number;
    let (start, first_flags) = parse_time(get("Start").trim(), timing_line, b".", false)?;
    let (end, second_flags) = parse_time(get("End").trim(), timing_line, b".", false)?;
    push_time_warnings(
        &mut warnings,
        timing_line,
        first_flags,
        second_flags,
        Format::Ass,
    );
    let marked_text = get("Marked").trim();
    let fields = AssFields {
        kind,
        layer: parse_i32_field(get("Layer"), "Layer", line_number, &mut warnings),
        marked: marked_text.strip_prefix("Marked=").unwrap_or(marked_text) == "1",
        style: get("Style").trim().to_owned(),
        name: get("Name").trim().to_owned(),
        margin_l: parse_i32_field(get("MarginL"), "MarginL", line_number, &mut warnings),
        margin_r: parse_i32_field(get("MarginR"), "MarginR", line_number, &mut warnings),
        margin_v: parse_i32_field(get("MarginV"), "MarginV", line_number, &mut warnings),
        effect: get("Effect").trim().to_owned(),
    };
    if end < start {
        warnings.push(Warning::EndBeforeStart { cue: cue_number });
    }
    if kind != EventKind::Dialogue {
        return Ok((None, warnings));
    }
    Ok((
        Some(Cue {
            index: None,
            id: None,
            start,
            end,
            text: get("Text").trim_start().to_owned(),
            settings: None,
            ass: Some(fields),
        }),
        warnings,
    ))
}

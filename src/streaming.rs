use std::collections::HashSet;
use std::io::BufRead;

use crate::model::{Cue, Error, Format, Header, Time, VttBlock, VttBlockKind, Warning};
use crate::parse::{
    is_known_section, is_position_suffix, is_timing_line, looks_like_time, parse_ass,
    parse_ass_stream_event, parse_event_format, parse_style_format, parse_style_line, parse_time,
    push_time_warnings, split_timing, style_name_set, timing_parses,
};

/// Warnings kept twice: once for `warnings()` and once for `take_warnings()`.
#[derive(Default)]
struct WarningSink {
    all: Vec<Warning>,
    pending: Vec<Warning>,
    scratch: Vec<Warning>,
}

impl WarningSink {
    fn push(&mut self, warning: Warning) {
        self.pending.push(warning.clone());
        self.all.push(warning);
    }

    fn drain_scratch(&mut self) {
        for warning in std::mem::take(&mut self.scratch) {
            self.push(warning);
        }
    }

    fn extend(&mut self, warnings: Vec<Warning>) {
        for warning in warnings {
            self.push(warning);
        }
    }
}

struct LineReader<R: BufRead> {
    reader: R,
    buffer: Vec<u8>,
    number: usize,
    offset: usize,
    line_offset: usize,
    endings: u8,
    mixed_reported: bool,
    first: bool,
    shared_warnings: Vec<Warning>,
}

impl<R: BufRead> LineReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::with_capacity(256),
            number: 0,
            offset: 0,
            line_offset: 0,
            endings: 0,
            mixed_reported: false,
            first: true,
            shared_warnings: Vec::new(),
        }
    }

    /// Reads one physical line into the reused buffer. Returns `Ok(false)`
    /// at end of input.
    fn read(&mut self) -> Result<bool, Error> {
        self.buffer.clear();
        self.line_offset = self.offset;
        let mut found = false;
        loop {
            let (length, ending) = {
                let available = match self.reader.fill_buf() {
                    Ok(available) => available,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(Error::Io(error)),
                };
                if available.is_empty() {
                    break;
                }
                match available
                    .iter()
                    .position(|byte| matches!(byte, b'\n' | b'\r'))
                {
                    Some(position) => {
                        self.buffer.extend_from_slice(&available[..position]);
                        (position + 1, Some(available[position]))
                    }
                    None => {
                        let length = available.len();
                        self.buffer.extend_from_slice(available);
                        (length, None)
                    }
                }
            };
            self.reader.consume(length);
            self.offset += length;
            if let Some(ending) = ending {
                found = true;
                if ending == b'\n' {
                    self.note_ending(1);
                } else {
                    let has_lf = loop {
                        match self.reader.fill_buf() {
                            Ok(available) => break available.first() == Some(&b'\n'),
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(error) => return Err(Error::Io(error)),
                        }
                    };
                    if has_lf {
                        self.reader.consume(1);
                        self.offset += 1;
                        self.note_ending(2);
                    } else {
                        self.note_ending(4);
                    }
                }
                break;
            }
        }
        if !found && self.buffer.is_empty() {
            return Ok(false);
        }
        self.number += 1;
        if self.first {
            self.first = false;
            if self.buffer.starts_with(&[0xff, 0xfe]) || self.buffer.starts_with(&[0xfe, 0xff]) {
                return Err(Error::Utf16Input);
            }
            if self.buffer.starts_with(&[0xef, 0xbb, 0xbf]) {
                self.buffer.drain(..3);
                self.shared_warnings.push(Warning::BomStripped);
            }
        }
        std::str::from_utf8(&self.buffer).map_err(|error| Error::InvalidUtf8 {
            offset: self.line_offset + error.valid_up_to(),
        })?;
        Ok(true)
    }

    fn note_ending(&mut self, ending: u8) {
        self.endings |= ending;
        if self.endings.count_ones() > 1 && !self.mixed_reported {
            self.mixed_reported = true;
            self.shared_warnings.push(Warning::MixedLineEndings);
        }
    }

    fn text(&self) -> &str {
        std::str::from_utf8(&self.buffer).expect("line validation is an invariant")
    }

    fn number(&self) -> usize {
        self.number
    }

    fn drain_shared(&mut self, sink: &mut WarningSink) {
        if !self.shared_warnings.is_empty() {
            sink.extend(std::mem::take(&mut self.shared_warnings));
        }
    }
}

fn is_index_like(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && trimmed.bytes().all(|byte| byte.is_ascii_digit())
}

/// SRT cue iterator.
///
/// The iterator owns one line buffer and one text accumulator, both reused
/// across cues. Each yielded cue allocates its own text once. A cue whose
/// timing line does not parse is yielded as `Err` and the lines up to the
/// next timing line are skipped.
pub struct SrtCues<R: BufRead> {
    lines: LineReader<R>,
    state: SrtState,
}

#[derive(Default)]
struct SrtState {
    text: String,
    held: String,
    held_line: Option<usize>,
    current: Option<OpenSrt>,
    deferred: Option<Error>,
    skipping: bool,
    seen_timing: bool,
    early_stray: Vec<usize>,
    /// The previous line was blank, an index line, or absent.
    previous_block_position: bool,
    expected_index: Option<u32>,
    sink: WarningSink,
    cue_count: usize,
    finished: bool,
}

struct OpenSrt {
    index: Option<u32>,
    start: Time,
    end: Time,
    settings: Option<String>,
    lines: usize,
    text_lines: usize,
    blank_seen: bool,
    empty: bool,
    stray_reported: bool,
}

impl<R: BufRead> SrtCues<R> {
    /// Creates an iterator over SRT cues.
    pub fn new(reader: R) -> Self {
        Self {
            lines: LineReader::new(reader),
            state: SrtState {
                text: String::with_capacity(256),
                held: String::with_capacity(16),
                previous_block_position: true,
                ..SrtState::default()
            },
        }
    }

    /// Returns warnings collected so far.
    pub fn warnings(&self) -> &[Warning] {
        &self.state.sink.all
    }

    /// Takes warnings collected since the previous call.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        std::mem::take(&mut self.state.sink.pending)
    }
}

impl SrtState {
    fn timing_line(&mut self, line: &str, number: usize) -> Option<Result<Cue, Error>> {
        if !self.seen_timing {
            self.seen_timing = true;
            for stray in std::mem::take(&mut self.early_stray) {
                self.sink.push(Warning::StrayText { line: stray });
            }
        }
        let boundary = self.held_line.unwrap_or(number);
        let closed = self
            .current
            .take()
            .map(|open| self.close_cue(open, Some(boundary)));
        let index = match self.held_line.take() {
            Some(held_line) => match self.held.trim().parse::<u32>() {
                Ok(found) => {
                    if let Some(expected) = self.expected_index {
                        if found != expected {
                            self.sink.push(Warning::NonSequentialIndex {
                                line: held_line,
                                expected,
                                found,
                            });
                        }
                    }
                    self.expected_index = found.checked_add(1);
                    Some(found)
                }
                Err(_) => {
                    self.sink.push(Warning::MissingIndex { line: number });
                    None
                }
            },
            None => {
                self.sink.push(Warning::MissingIndex { line: number });
                None
            }
        };
        self.held.clear();
        match self.open_cue(index, line, number) {
            Ok(open) => {
                self.current = Some(open);
                self.skipping = false;
                closed.map(Ok)
            }
            Err(error) => {
                self.skipping = true;
                match closed {
                    Some(cue) => {
                        self.deferred = Some(error);
                        Some(Ok(cue))
                    }
                    None => Some(Err(error)),
                }
            }
        }
    }

    fn open_cue(
        &mut self,
        index: Option<u32>,
        line: &str,
        number: usize,
    ) -> Result<OpenSrt, Error> {
        let (start_text, end_text, suffix) = split_timing(line, number)?;
        let (start, start_flags) = parse_time(start_text, number, b",.", true)?;
        let (end, end_flags) = parse_time(end_text, number, b",.", true)?;
        push_time_warnings(
            &mut self.sink.scratch,
            number,
            start_flags,
            end_flags,
            Format::Srt,
        );
        self.sink.drain_scratch();
        let settings = if suffix.is_empty() {
            None
        } else {
            if !is_position_suffix(suffix) {
                self.sink
                    .push(Warning::UnknownTimingSuffix { line: number });
            }
            Some(suffix.to_owned())
        };
        if end < start {
            self.sink.push(Warning::EndBeforeStart {
                cue: self.cue_count + 1,
            });
        }
        Ok(OpenSrt {
            index,
            start,
            end,
            settings,
            lines: 0,
            text_lines: 0,
            blank_seen: false,
            empty: false,
            stray_reported: false,
        })
    }

    fn close_cue(&mut self, open: OpenSrt, boundary: Option<usize>) -> Cue {
        let cue = self.cue_count + 1;
        if open.lines == 0 {
            self.sink.push(Warning::EmptyCue { cue });
        }
        if let Some(line) = boundary {
            if !open.blank_seen && open.lines > 0 {
                self.sink.push(Warning::MissingBlankLine { line });
            }
        }
        let text = self.text.clone();
        self.text.clear();
        self.cue_count += 1;
        Cue {
            index: open.index,
            id: None,
            start: open.start,
            end: open.end,
            text,
            settings: open.settings,
            ass: None,
        }
    }

    fn other_line(&mut self, line: &str, number: usize) -> Option<Error> {
        if !self.seen_timing && looks_like_time(line) && self.previous_block_position {
            return Some(Error::MissingArrow { line: number });
        }
        self.release_held();
        if is_index_like(line) {
            self.held.clear();
            self.held.push_str(line);
            self.held_line = Some(number);
        } else {
            self.feed(line, number);
        }
        None
    }

    fn release_held(&mut self) {
        if let Some(number) = self.held_line.take() {
            let held = std::mem::take(&mut self.held);
            self.feed(&held, number);
            self.held = held;
        }
    }

    fn feed(&mut self, line: &str, number: usize) {
        let cue = self.cue_count + 1;
        let Some(open) = self.current.as_mut() else {
            if !self.skipping && !self.seen_timing {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
                    self.early_stray.push(number);
                }
            }
            return;
        };
        open.lines += 1;
        if open.lines == 1 && line.is_empty() {
            open.empty = true;
            open.blank_seen = true;
            self.sink.push(Warning::EmptyCue { cue });
            return;
        }
        if open.empty {
            if !line.is_empty() && !open.stray_reported {
                open.stray_reported = true;
                self.sink.push(Warning::StrayText { line: number });
            }
            return;
        }
        if !open.blank_seen {
            if line.is_empty() {
                open.blank_seen = true;
            } else {
                if open.text_lines > 0 {
                    self.text.push('\n');
                }
                self.text.push_str(line);
                open.text_lines += 1;
            }
            return;
        }
        if line.trim().is_empty() {
            return;
        }
        self.sink.push(Warning::StrayText { line: number });
        if is_index_like(line) {
            self.text.push_str("\n\n");
            self.text.push_str(line);
        }
    }

    fn finish(&mut self) {
        if !self.finished {
            self.finished = true;
            if self.cue_count == 0 {
                self.sink.push(Warning::NoCues);
            }
        }
    }

    /// A cue whose text ended at a blank line is complete, so it is yielded
    /// before the error. A cue still collecting text is dropped.
    fn read_failed(&mut self, error: Error) -> Result<Cue, Error> {
        match self.current.take() {
            Some(open) if open.blank_seen => {
                self.deferred = Some(error);
                Ok(self.close_cue(open, None))
            }
            _ => Err(error),
        }
    }
}

impl<R: BufRead> Iterator for SrtCues<R> {
    type Item = Result<Cue, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(error) = self.state.deferred.take() {
            return Some(Err(error));
        }
        if self.state.finished {
            return None;
        }
        loop {
            match self.lines.read() {
                Ok(true) => {}
                Ok(false) => {
                    self.state.release_held();
                    let closed = self
                        .state
                        .current
                        .take()
                        .map(|open| self.state.close_cue(open, None));
                    self.state.finish();
                    return closed.map(Ok);
                }
                Err(error) => {
                    self.state.finished = true;
                    return Some(self.state.read_failed(error));
                }
            }
            self.lines.drain_shared(&mut self.state.sink);
            let number = self.lines.number();
            let line = self.lines.text();
            let timing = is_timing_line(line, self.state.previous_block_position);
            let item = if timing {
                self.state.timing_line(line, number)
            } else {
                self.state.other_line(line, number).map(Err)
            };
            self.state.previous_block_position =
                line.trim().bytes().all(|byte| byte.is_ascii_digit());
            if item.is_some() {
                return item;
            }
        }
    }
}

/// WebVTT cue iterator.
///
/// The constructor reads the signature and header block. Cues, and the
/// metadata blocks between them, are read one line at a time with one line
/// buffer and one text accumulator. Blocks are appended to the header as
/// they are met, so [`VttCues::header`] is complete after iteration.
pub struct VttCues<R: BufRead> {
    lines: LineReader<R>,
    state: VttState,
}

#[derive(Default)]
struct VttState {
    text: String,
    header: Header,
    block: VttBlockState,
    deferred: Option<Error>,
    sink: WarningSink,
    cue_count: usize,
    reuse_line: bool,
    finished: bool,
}

#[derive(Default)]
enum VttBlockState {
    #[default]
    Idle,
    Id {
        id: String,
        line: usize,
    },
    Meta(VttBlockKind),
    Cue(OpenVtt),
    Skip,
}

struct OpenVtt {
    id: Option<String>,
    start: Time,
    end: Time,
    settings: Option<String>,
    text_lines: usize,
}

impl<R: BufRead> VttCues<R> {
    /// Reads the WebVTT signature and header, then creates the iterator.
    pub fn new(reader: R) -> Result<Self, Error> {
        let mut lines = LineReader::new(reader);
        let mut state = VttState {
            text: String::with_capacity(256),
            ..VttState::default()
        };
        if !lines.read()? {
            return Err(Error::MissingHeader {
                line: 1,
                found: String::new(),
            });
        }
        lines.drain_shared(&mut state.sink);
        let first = lines.text();
        let signature = first
            .strip_prefix("WEBVTT")
            .filter(|suffix| {
                suffix.is_empty() || suffix.starts_with(' ') || suffix.starts_with('\t')
            })
            .ok_or_else(|| Error::MissingHeader {
                line: 1,
                found: first.to_owned(),
            })?;
        let signature = signature.trim();
        state.header.vtt_signature = (!signature.is_empty()).then(|| signature.to_owned());
        while lines.read()? {
            lines.drain_shared(&mut state.sink);
            let text = lines.text();
            if text.is_empty() {
                break;
            }
            if text.contains("-->") {
                state.sink.push(Warning::MissingBlankLine {
                    line: lines.number(),
                });
                state.reuse_line = true;
                break;
            }
            state.header.vtt_header_lines.push(text.to_owned());
        }
        Ok(Self { lines, state })
    }

    /// Returns the parsed WebVTT header.
    pub fn header(&self) -> &Header {
        &self.state.header
    }

    /// Returns warnings collected so far.
    pub fn warnings(&self) -> &[Warning] {
        &self.state.sink.all
    }

    /// Takes warnings collected since the previous call.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        std::mem::take(&mut self.state.sink.pending)
    }
}

impl VttState {
    fn line(&mut self, text: &str, number: usize) -> Option<Result<Cue, Error>> {
        match std::mem::take(&mut self.block) {
            VttBlockState::Idle => {
                if text.is_empty() {
                    return None;
                }
                if let Some(kind) = block_kind(text) {
                    self.text.clear();
                    self.text.push_str(text);
                    self.block = VttBlockState::Meta(kind);
                    return None;
                }
                if !text.contains("-->") {
                    self.block = VttBlockState::Id {
                        id: text.to_owned(),
                        line: number,
                    };
                    return None;
                }
                self.start_cue(None, text, number)
            }
            VttBlockState::Id { id, line } => {
                if !text.contains("-->") {
                    self.sink.push(Warning::StrayText { line });
                    if !text.is_empty() {
                        self.block = VttBlockState::Skip;
                    }
                    return None;
                }
                self.start_cue(Some(id), text, number)
            }
            VttBlockState::Meta(kind) => {
                if text.is_empty() {
                    self.push_block(kind);
                } else {
                    self.text.push('\n');
                    self.text.push_str(text);
                    self.block = VttBlockState::Meta(kind);
                }
                None
            }
            VttBlockState::Cue(mut open) => {
                if text.is_empty() {
                    return Some(Ok(self.close_cue(open)));
                }
                if text.contains("-->") && timing_parses(text) {
                    let closed = self.close_cue(open);
                    self.sink.push(Warning::MissingBlankLine { line: number });
                    if let Some(Err(error)) = self.start_cue(None, text, number) {
                        self.deferred = Some(error);
                    }
                    return Some(Ok(closed));
                }
                if open.text_lines > 0 {
                    self.text.push('\n');
                }
                self.text.push_str(text);
                open.text_lines += 1;
                self.block = VttBlockState::Cue(open);
                None
            }
            VttBlockState::Skip => {
                if !text.is_empty() {
                    self.block = VttBlockState::Skip;
                }
                None
            }
        }
    }

    fn start_cue(
        &mut self,
        id: Option<String>,
        line: &str,
        number: usize,
    ) -> Option<Result<Cue, Error>> {
        match self.open_cue(id, line, number) {
            Ok(open) => {
                self.block = VttBlockState::Cue(open);
                None
            }
            Err(error) => {
                self.block = VttBlockState::Skip;
                Some(Err(error))
            }
        }
    }

    fn open_cue(
        &mut self,
        id: Option<String>,
        line: &str,
        number: usize,
    ) -> Result<OpenVtt, Error> {
        let (start_text, end_text, suffix) = split_timing(line, number)?;
        let (start, start_flags) = parse_time(start_text, number, b".,", true)?;
        let (end, end_flags) = parse_time(end_text, number, b".,", true)?;
        push_time_warnings(
            &mut self.sink.scratch,
            number,
            start_flags,
            end_flags,
            Format::Vtt,
        );
        self.sink.drain_scratch();
        if end < start {
            self.sink.push(Warning::EndBeforeStart {
                cue: self.cue_count + 1,
            });
        }
        Ok(OpenVtt {
            id,
            start,
            end,
            settings: (!suffix.is_empty()).then(|| suffix.to_owned()),
            text_lines: 0,
        })
    }

    fn close_cue(&mut self, open: OpenVtt) -> Cue {
        let text = self.text.clone();
        self.text.clear();
        self.cue_count += 1;
        Cue {
            index: None,
            id: open.id,
            start: open.start,
            end: open.end,
            text,
            settings: open.settings,
            ass: None,
        }
    }

    fn push_block(&mut self, kind: VttBlockKind) {
        self.header.blocks.push(VttBlock {
            kind,
            raw: self.text.clone(),
            cue_index: self.cue_count,
        });
        self.text.clear();
    }

    fn end_of_input(&mut self) -> Option<Result<Cue, Error>> {
        let item = match std::mem::take(&mut self.block) {
            VttBlockState::Meta(kind) => {
                self.push_block(kind);
                None
            }
            VttBlockState::Cue(open) => Some(Ok(self.close_cue(open))),
            VttBlockState::Id { line, .. } => {
                self.sink.push(Warning::StrayText { line });
                None
            }
            VttBlockState::Idle | VttBlockState::Skip => None,
        };
        self.finished = true;
        if self.cue_count == 0 {
            self.sink.push(Warning::NoCues);
        }
        item
    }
}

fn block_kind(first_line: &str) -> Option<VttBlockKind> {
    if first_line == "REGION" {
        Some(VttBlockKind::Region)
    } else if first_line == "STYLE" {
        Some(VttBlockKind::Style)
    } else if first_line == "NOTE"
        || first_line.starts_with("NOTE ")
        || first_line.starts_with("NOTE\t")
    {
        Some(VttBlockKind::Note)
    } else {
        None
    }
}

impl<R: BufRead> Iterator for VttCues<R> {
    type Item = Result<Cue, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(error) = self.state.deferred.take() {
            return Some(Err(error));
        }
        if self.state.finished {
            return None;
        }
        loop {
            if self.state.reuse_line {
                self.state.reuse_line = false;
            } else {
                match self.lines.read() {
                    Ok(true) => self.lines.drain_shared(&mut self.state.sink),
                    Ok(false) => return self.state.end_of_input(),
                    Err(error) => {
                        self.state.finished = true;
                        return Some(Err(error));
                    }
                }
            }
            let number = self.lines.number();
            let item = self.state.line(self.lines.text(), number);
            if item.is_some() {
                return item;
            }
        }
    }
}

/// ASS cue iterator.
///
/// The constructor reads every line before the first event and parses the
/// script information and styles from it. Events are then read one line at
/// a time. A Dialogue line whose timing does not parse is yielded as `Err`
/// and iteration continues with the next line. A styles section that
/// follows the first event is read as it is met and added to the header,
/// which also withdraws a `MissingStyles` warning. Cues yielded before it
/// keep any `UnknownStyle` warning they were given.
pub struct AssCues<R: BufRead> {
    lines: LineReader<R>,
    header: Header,
    style_names: HashSet<String>,
    event_format: Option<Vec<String>>,
    style_format: Option<Vec<String>>,
    section: AssSection,
    pending_event: bool,
    warned_missing_format: bool,
    sink: WarningSink,
    cue_count: usize,
    finished: bool,
}

#[derive(Clone, Copy)]
enum AssSection {
    Other,
    Styles { v4: bool },
    Events,
}

impl<R: BufRead> AssCues<R> {
    /// Reads ASS script and style metadata, then creates the iterator.
    pub fn new(reader: R) -> Result<Self, Error> {
        let mut lines = LineReader::new(reader);
        let mut prefix = Vec::with_capacity(2048);
        let mut in_events = false;
        let mut event_format = None;
        let mut pending_event = false;
        let mut sink = WarningSink::default();
        while lines.read()? {
            let trimmed = lines.text().trim();
            if let Some(name) = section_name(trimmed) {
                in_events = name.eq_ignore_ascii_case("Events");
            } else if in_events {
                if let Some(value) = key_value(trimmed, "Format") {
                    let mut ignored = Vec::new();
                    event_format = Some(parse_event_format(value, &mut ignored));
                } else if is_event_line(trimmed) {
                    pending_event = true;
                    break;
                }
            }
            prefix.extend_from_slice(&lines.buffer);
            prefix.push(b'\n');
        }
        let subtitles = parse_ass(&prefix)?;
        lines.drain_shared(&mut sink);
        sink.extend(
            subtitles
                .warnings
                .into_iter()
                .filter(|warning| !matches!(warning, Warning::NoCues))
                .collect(),
        );
        Ok(Self {
            lines,
            style_names: style_name_set(&subtitles.header.styles),
            header: subtitles.header,
            event_format,
            style_format: None,
            section: if in_events {
                AssSection::Events
            } else {
                AssSection::Other
            },
            pending_event,
            warned_missing_format: false,
            sink,
            cue_count: 0,
            finished: false,
        })
    }

    /// Returns the parsed ASS header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Returns warnings collected so far.
    pub fn warnings(&self) -> &[Warning] {
        &self.sink.all
    }

    /// Takes warnings collected since the previous call.
    pub fn take_warnings(&mut self) -> Vec<Warning> {
        std::mem::take(&mut self.sink.pending)
    }

    fn finish(&mut self) {
        if !self.finished {
            self.finished = true;
            self.lines.drain_shared(&mut self.sink);
            if self.cue_count == 0 {
                self.sink.push(Warning::NoCues);
            }
        }
    }

    fn late_style_line(&mut self, v4: bool) {
        let line = self.lines.text().trim();
        if let Some(value) = key_value(line, "Format") {
            self.style_format = Some(parse_style_format(value));
            return;
        }
        let Some(value) = key_value(line, "Style") else {
            return;
        };
        let style = parse_style_line(
            value,
            self.style_format.as_deref(),
            v4,
            self.lines.number(),
            &mut self.sink.scratch,
        );
        self.sink.drain_scratch();
        self.style_names.insert(style.name.to_ascii_lowercase());
        self.header.styles.push(style);
        let missing = |warning: &Warning| !matches!(warning, Warning::MissingStyles);
        self.sink.all.retain(missing);
        self.sink.pending.retain(missing);
        if v4 && !self.sink.all.contains(&Warning::UpgradedToV4Plus) {
            self.sink.push(Warning::UpgradedToV4Plus);
            for (key, value) in &mut self.header.script_info {
                if key.eq_ignore_ascii_case("ScriptType") {
                    *value = "v4.00+".to_owned();
                }
            }
        }
    }

    fn check_style(&mut self, cue: &Cue) {
        let Some(fields) = &cue.ass else {
            return;
        };
        if self.header.styles.is_empty()
            || self
                .style_names
                .contains(&fields.style.to_ascii_lowercase())
        {
            return;
        }
        self.sink.push(Warning::UnknownStyle {
            cue: self.cue_count,
            style: fields.style.clone(),
        });
    }
}

impl<R: BufRead> Iterator for AssCues<R> {
    type Item = Result<Cue, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        loop {
            if self.pending_event {
                self.pending_event = false;
            } else {
                match self.lines.read() {
                    Ok(false) => {
                        self.finish();
                        return None;
                    }
                    Ok(true) => {}
                    Err(error) => {
                        self.finished = true;
                        return Some(Err(error));
                    }
                }
            }
            self.lines.drain_shared(&mut self.sink);
            let trimmed = self.lines.text().trim();
            if let Some(name) = section_name(trimmed) {
                let lower = name.to_ascii_lowercase();
                self.section = match lower.as_str() {
                    "events" => AssSection::Events,
                    "v4+ styles" => AssSection::Styles { v4: false },
                    "v4 styles" => AssSection::Styles { v4: true },
                    _ => AssSection::Other,
                };
                self.style_format = None;
                if !is_known_section(&lower) {
                    self.sink.push(Warning::UnknownSection {
                        line: self.lines.number(),
                        name: name.to_owned(),
                    });
                }
                continue;
            }
            match self.section {
                AssSection::Other => continue,
                AssSection::Styles { v4 } => {
                    self.late_style_line(v4);
                    continue;
                }
                AssSection::Events => {}
            }
            if let Some(value) = key_value(trimmed, "Format") {
                self.event_format = Some(parse_event_format(value, &mut self.sink.scratch));
                self.sink.drain_scratch();
                continue;
            }
            if self.event_format.is_none() && !self.warned_missing_format && is_event_line(trimmed)
            {
                self.warned_missing_format = true;
                self.sink.push(Warning::MissingFormatLine {
                    section: "Events".to_owned(),
                });
            }
            let result = parse_ass_stream_event(
                self.lines.text(),
                self.lines.number(),
                self.event_format.as_deref(),
                self.cue_count + 1,
            );
            match result {
                Ok((Some(cue), warnings)) => {
                    self.cue_count += 1;
                    self.sink.extend(warnings);
                    self.check_style(&cue);
                    return Some(Ok(cue));
                }
                Ok((None, warnings)) => self.sink.extend(warnings),
                Err(error) => return Some(Err(error)),
            }
        }
    }
}

fn section_name(line: &str) -> Option<&str> {
    line.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (found, value) = line.split_once(':')?;
    found
        .trim()
        .eq_ignore_ascii_case(key)
        .then(|| value.trim_start())
}

fn is_event_line(line: &str) -> bool {
    [
        "Dialogue", "Comment", "Picture", "Sound", "Movie", "Command",
    ]
    .iter()
    .any(|key| key_value(line, key).is_some())
}

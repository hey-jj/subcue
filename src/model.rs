use std::fmt;

/// A supported subtitle format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Format {
    /// SubRip subtitles.
    Srt,
    /// Web Video Text Tracks.
    Vtt,
    /// Advanced SubStation Alpha or SubStation Alpha.
    Ass,
}

/// Signed milliseconds from the start of the media timeline.
///
/// Parsing never produces a negative value for SRT or WebVTT. A negative
/// value set by a caller composes as zero in every format, because none of
/// the three formats can express it.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Time(pub i64);

/// Parsed subtitles and format-specific metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subtitles {
    /// The text dialect used by every cue in this value.
    pub format: Format,
    /// File-level metadata.
    pub header: Header,
    /// Timed visible cues in file order.
    pub cues: Vec<Cue>,
    /// Non-dialogue ASS events in file order.
    pub events: Vec<AssEvent>,
    /// Recoverable conditions found during parsing or conversion.
    pub warnings: Vec<Warning>,
}

/// One timed subtitle cue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cue {
    /// The SRT index, when the source supplied one.
    pub index: Option<u32>,
    /// The WebVTT cue identifier, when present.
    pub id: Option<String>,
    /// Cue start time.
    pub start: Time,
    /// Cue end time.
    pub end: Time,
    /// Cue text in the dialect named by [`Subtitles::format`].
    pub text: String,
    /// Raw SRT timing suffix or WebVTT cue settings.
    pub settings: Option<String>,
    /// ASS event fields for ASS cues.
    pub ass: Option<AssFields>,
}

/// Format-specific fields from an ASS dialogue event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssFields {
    /// Event kind.
    pub kind: EventKind,
    /// ASS layer number.
    pub layer: i32,
    /// SSA marked flag.
    pub marked: bool,
    /// Style name.
    pub style: String,
    /// Actor or speaker name.
    pub name: String,
    /// Left margin override.
    pub margin_l: i32,
    /// Right margin override.
    pub margin_r: i32,
    /// Vertical margin override.
    pub margin_v: i32,
    /// Raw effect field.
    pub effect: String,
}

impl Default for AssFields {
    fn default() -> Self {
        Self {
            kind: EventKind::Dialogue,
            layer: 0,
            marked: false,
            style: "Default".to_owned(),
            name: String::new(),
            margin_l: 0,
            margin_r: 0,
            margin_v: 0,
            effect: String::new(),
        }
    }
}

/// An ASS event type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EventKind {
    /// Visible dialogue.
    Dialogue,
    /// An editor comment event.
    Comment,
    /// A picture event.
    Picture,
    /// A sound event.
    Sound,
    /// A movie event.
    Movie,
    /// A command event.
    Command,
}

impl EventKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Dialogue => "Dialogue",
            Self::Comment => "Comment",
            Self::Picture => "Picture",
            Self::Sound => "Sound",
            Self::Movie => "Movie",
            Self::Command => "Command",
        }
    }
}

/// A non-dialogue ASS event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssEvent {
    /// Event kind.
    pub kind: EventKind,
    /// Layer number.
    pub layer: i32,
    /// SSA marked flag.
    pub marked: bool,
    /// Start time.
    pub start: Time,
    /// End time.
    pub end: Time,
    /// Style name.
    pub style: String,
    /// Actor or speaker name.
    pub name: String,
    /// Left margin override.
    pub margin_l: i32,
    /// Right margin override.
    pub margin_r: i32,
    /// Vertical margin override.
    pub margin_v: i32,
    /// Raw effect field.
    pub effect: String,
    /// Raw event text.
    pub text: String,
    /// Position among all event lines.
    pub event_index: usize,
}

/// File-level metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Header {
    /// Text following the WebVTT signature.
    pub vtt_signature: Option<String>,
    /// WebVTT header lines in file order.
    pub vtt_header_lines: Vec<String>,
    /// WebVTT metadata blocks in file order.
    pub blocks: Vec<VttBlock>,
    /// ASS script information pairs in file order.
    pub script_info: Vec<(String, String)>,
    /// Typed ASS styles in file order.
    pub styles: Vec<Style>,
    /// ASS sections whose bodies stay raw.
    pub raw_sections: Vec<RawSection>,
}

/// A WebVTT metadata block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VttBlock {
    /// Block kind.
    pub kind: VttBlockKind,
    /// Complete block text without the separating blank line.
    pub raw: String,
    /// Number of cues that precede this block.
    pub cue_index: usize,
}

/// A WebVTT metadata block type.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum VttBlockKind {
    /// A region block.
    Region,
    /// A style block.
    Style,
    /// A note block.
    Note,
}

/// An ASS section preserved as text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawSection {
    /// Section name without brackets.
    pub name: String,
    /// Section body lines.
    pub lines: Vec<String>,
    /// Zero-based position among every section of the file. Compose writes
    /// the section back at this position, with the script information,
    /// styles, and events sections filling the positions between raw
    /// sections in that order.
    pub position: usize,
}

/// A base-10 number used by ASS style fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct AssNumber {
    units: i64,
    scale: u32,
}

impl AssNumber {
    /// Creates an integer ASS number.
    pub const fn from_integer(value: i64) -> Self {
        Self {
            units: value,
            scale: 0,
        }
    }

    pub(crate) fn parse(input: &str) -> Option<Self> {
        let input = input.trim();
        if input.is_empty() {
            return None;
        }
        let (negative, rest) = match input.as_bytes()[0] {
            b'-' => (true, &input[1..]),
            b'+' => (false, &input[1..]),
            _ => (false, input),
        };
        let (whole, fraction) = rest.split_once('.').unwrap_or((rest, ""));
        if whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let scale = fraction.len() as u32;
        let factor = 10_i64.checked_pow(scale)?;
        let mut units = whole.parse::<i64>().ok()?.checked_mul(factor)?;
        if !fraction.is_empty() {
            units = units.checked_add(fraction.parse::<i64>().ok()?)?;
        }
        if negative {
            units = units.checked_neg()?;
        }
        Some(Self { units, scale }.normalized())
    }

    fn normalized(mut self) -> Self {
        while self.scale > 0 && self.units % 10 == 0 {
            self.units /= 10;
            self.scale -= 1;
        }
        self
    }
}

impl fmt::Display for AssNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale == 0 {
            return write!(f, "{}", self.units);
        }
        let negative = self.units < 0;
        let magnitude = self.units.unsigned_abs();
        let factor = 10_u64.pow(self.scale);
        if negative {
            f.write_str("-")?;
        }
        write!(
            f,
            "{}.{:0width$}",
            magnitude / factor,
            magnitude % factor,
            width = self.scale as usize
        )
    }
}

/// An ASS colour encoded as `&HAABBGGRR`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct Colour(pub u32);

impl fmt::Display for Colour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "&H{:08X}", self.0)
    }
}

/// A typed ASS v4+ style.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Style {
    /// Style name.
    pub name: String,
    /// Font family.
    pub font_name: String,
    /// Font size.
    pub font_size: AssNumber,
    /// Primary colour.
    pub primary_colour: Colour,
    /// Secondary colour.
    pub secondary_colour: Colour,
    /// Outline colour.
    pub outline_colour: Colour,
    /// Background colour.
    pub back_colour: Colour,
    /// Bold flag.
    pub bold: bool,
    /// Italic flag.
    pub italic: bool,
    /// Underline flag.
    pub underline: bool,
    /// Strikeout flag.
    pub strike_out: bool,
    /// Horizontal scale.
    pub scale_x: AssNumber,
    /// Vertical scale.
    pub scale_y: AssNumber,
    /// Character spacing.
    pub spacing: AssNumber,
    /// Rotation angle.
    pub angle: AssNumber,
    /// Border style number.
    pub border_style: i32,
    /// Outline width.
    pub outline: AssNumber,
    /// Shadow distance.
    pub shadow: AssNumber,
    /// Alignment number.
    pub alignment: i32,
    /// Left margin.
    pub margin_l: i32,
    /// Right margin.
    pub margin_r: i32,
    /// Vertical margin.
    pub margin_v: i32,
    /// Font encoding number.
    pub encoding: i32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            name: "Default".to_owned(),
            font_name: "Arial".to_owned(),
            font_size: AssNumber::from_integer(20),
            primary_colour: Colour(0x00ff_ffff),
            secondary_colour: Colour(0x0000_00ff),
            outline_colour: Colour(0),
            back_colour: Colour(0),
            bold: false,
            italic: false,
            underline: false,
            strike_out: false,
            scale_x: AssNumber::from_integer(100),
            scale_y: AssNumber::from_integer(100),
            spacing: AssNumber::from_integer(0),
            angle: AssNumber::from_integer(0),
            border_style: 1,
            outline: AssNumber::from_integer(2),
            shadow: AssNumber::from_integer(0),
            alignment: 2,
            margin_l: 10,
            margin_r: 10,
            margin_v: 10,
            encoding: 1,
        }
    }
}

/// A recoverable parse, compose, or conversion condition.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Warning {
    /// A UTF-8 byte order mark was removed.
    BomStripped,
    /// More than one line-ending form occurred.
    MixedLineEndings,
    /// No timed cues were found.
    NoCues,
    /// Minutes or seconds exceeded 59.
    ComponentOutOfRange {
        /// Physical input line.
        line: usize,
    },
    /// A cue ends before it starts.
    EndBeforeStart {
        /// One-based cue number.
        cue: usize,
    },
    /// An SRT cue has no index.
    MissingIndex {
        /// Physical input line.
        line: usize,
    },
    /// An SRT index differs from its file position.
    NonSequentialIndex {
        /// Physical input line.
        line: usize,
        /// Index implied by file order.
        expected: u32,
        /// Index read from the file.
        found: u32,
    },
    /// Text outside an SRT cue was ignored.
    StrayText {
        /// Physical input line.
        line: usize,
    },
    /// An SRT cue has no text.
    EmptyCue {
        /// One-based cue number.
        cue: usize,
    },
    /// An SRT timestamp used a dot before milliseconds.
    DotMillis {
        /// Physical input line.
        line: usize,
    },
    /// A timestamp used fewer than three millisecond digits.
    ShortMillis {
        /// Physical input line.
        line: usize,
    },
    /// An SRT timestamp omitted hours.
    MissingHours {
        /// Physical input line.
        line: usize,
    },
    /// A required blank separator was absent.
    MissingBlankLine {
        /// Physical input line.
        line: usize,
    },
    /// An SRT timing suffix was kept without interpretation.
    UnknownTimingSuffix {
        /// Physical input line.
        line: usize,
    },
    /// A WebVTT timestamp used a comma before milliseconds.
    CommaMillis {
        /// Physical input line.
        line: usize,
    },
    /// A WebVTT timestamp used one hour digit.
    ShortHours {
        /// Physical input line.
        line: usize,
    },
    /// An unrecognized ASS section was kept.
    UnknownSection {
        /// Physical input line.
        line: usize,
        /// Section name.
        name: String,
    },
    /// An ASS row contained a different field count.
    FieldCountMismatch {
        /// Physical input line.
        line: usize,
        /// Field count declared by the format row.
        expected: usize,
        /// Field count read from the data row.
        found: usize,
    },
    /// An ASS section lacked a format row.
    MissingFormatLine {
        /// Section using its default field order.
        section: String,
    },
    /// An ASS timestamp used other than two fraction digits.
    AssFractionDigits {
        /// Physical input line.
        line: usize,
    },
    /// An ASS cue names an undefined style.
    UnknownStyle {
        /// One-based cue number.
        cue: usize,
        /// Unresolved style name.
        style: String,
    },
    /// An ASS field could not be parsed and used its default.
    BadField {
        /// Physical input line.
        line: usize,
        /// Field name.
        field: String,
    },
    /// SSA v4 data was normalized to ASS v4+.
    UpgradedToV4Plus,
    /// The ASS file had no styles section.
    MissingStyles,
    /// An ASS event format used another field order.
    FormatReordered,
    /// Milliseconds were truncated to ASS centiseconds.
    TimePrecisionLost {
        /// One-based cue number.
        cue: usize,
    },
    /// A text tag has no target-format representation.
    LossyTag {
        /// One-based cue number.
        cue: usize,
        /// Tag without its delimiters.
        tag: String,
    },
    /// Cue settings have no target-format representation.
    DroppedSettings {
        /// One-based cue number.
        cue: usize,
    },
    /// A cue identifier has no target-format representation.
    DroppedId {
        /// One-based cue number.
        cue: usize,
    },
    /// A WebVTT metadata block has no target-format representation.
    DroppedBlock {
        /// Block kind.
        kind: VttBlockKind,
    },
    /// A non-dialogue event has no target-format representation.
    DroppedEvent {
        /// Zero-based ASS event position.
        index: usize,
    },
}

impl Warning {
    pub(crate) fn is_output_warning(&self) -> bool {
        matches!(
            self,
            Self::TimePrecisionLost { .. }
                | Self::LossyTag { .. }
                | Self::DroppedSettings { .. }
                | Self::DroppedId { .. }
                | Self::DroppedBlock { .. }
                | Self::DroppedEvent { .. }
        )
    }
}

/// A fatal input error.
///
/// Every variant except [`Error::Io`] describes the input bytes. `Io` is
/// produced only by the streaming iterators when the underlying reader fails.
#[derive(Debug)]
pub enum Error {
    /// The input starts with a UTF-16 byte order mark.
    Utf16Input,
    /// The input is not valid UTF-8.
    InvalidUtf8 {
        /// Byte offset of the invalid sequence.
        offset: usize,
    },
    /// A timestamp integer exceeded the time model.
    TimeOutOfRange {
        /// Physical input line.
        line: usize,
    },
    /// A format that requires nonnegative time contained a negative value.
    NegativeTime {
        /// Physical input line.
        line: usize,
    },
    /// A timing component could not be read.
    InvalidTimestamp {
        /// Physical input line.
        line: usize,
    },
    /// An SRT time-shaped line lacked the required arrow.
    MissingArrow {
        /// Physical input line.
        line: usize,
    },
    /// A WebVTT signature was absent.
    MissingHeader {
        /// Physical input line.
        line: usize,
        /// First line found in the input.
        found: String,
    },
    /// Content appeared before any ASS section.
    NotAss {
        /// Physical input line.
        line: usize,
    },
    /// The streaming reader failed before the input could be decoded.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf16Input => f.write_str("UTF-16 input requires transcoding"),
            Self::InvalidUtf8 { offset } => write!(f, "invalid UTF-8 at byte {offset}"),
            Self::TimeOutOfRange { line } => write!(f, "time is out of range on line {line}"),
            Self::NegativeTime { line } => write!(f, "negative time on line {line}"),
            Self::InvalidTimestamp { line } => write!(f, "invalid timestamp on line {line}"),
            Self::MissingArrow { line } => write!(f, "missing timing arrow on line {line}"),
            Self::MissingHeader { line, found } => {
                write!(f, "missing WebVTT header on line {line}: {found}")
            }
            Self::NotAss { line } => write!(f, "content before an ASS section on line {line}"),
            Self::Io(error) => write!(f, "read failed: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Output line ending.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum LineEnding {
    /// Line feed.
    #[default]
    Lf,
    /// Carriage return followed by line feed.
    CrLf,
}

/// Options for composing subtitle bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct ComposeOptions {
    /// Line ending written between output lines.
    pub line_ending: LineEnding,
    /// Whether to prefix the output with a UTF-8 byte order mark.
    pub bom: bool,
}

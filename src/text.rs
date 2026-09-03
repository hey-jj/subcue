//! Inline cue text model shared by every format conversion.
//!
//! [`parse`] reads the tags of one dialect into a [`Span`] tree and
//! [`render`] writes a tree back in a dialect. A tag the target dialect
//! cannot express is dropped and reported by name in [`Rendered::dropped`],
//! so the converter can warn once per tag per cue.

use crate::model::Format;

/// An RGB colour carried by a colour tag.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Rgb {
    /// Red component.
    pub red: u8,
    /// Green component.
    pub green: u8,
    /// Blue component.
    pub blue: u8,
}

/// One node of inline cue text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Span {
    /// Literal text without line breaks.
    Text(String),
    /// Bold children.
    Bold(Vec<Span>),
    /// Italic children.
    Italic(Vec<Span>),
    /// Underlined children.
    Underline(Vec<Span>),
    /// Struck-through children.
    Strike(Vec<Span>),
    /// Coloured children.
    Color(Rgb, Vec<Span>),
    /// A line break.
    LineBreak,
    /// A source tag without delimiters that no other dialect can express.
    Raw(String),
}

/// Cue text parsed into spans plus the cue-level values some tags carry.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParsedText {
    /// The span tree.
    pub spans: Vec<Span>,
    /// ASS alignment from an `{\anN}` override, when one was present.
    pub alignment: Option<u8>,
    /// WebVTT speaker from a `<v name>` tag, when one was present.
    pub voice: Option<String>,
}

/// Rendered cue text plus the tags the dialect could not express.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Rendered {
    /// Text in the requested dialect.
    pub text: String,
    /// Dropped tags without delimiters, in order of first appearance.
    pub dropped: Vec<String>,
}

/// Parses cue text written in `dialect` into spans.
///
/// SRT and WebVTT tags nest. A closing tag that crosses an open one closes
/// the inner tags first and reopens them after it. ASS override commands
/// toggle state, so `{\b1}` opens a bold span and `{\b0}` closes it.
/// Unknown SRT and WebVTT tags stay literal text, and so does a `<` that
/// is not followed by a `>` before the next `<`. A closing tag whose kind
/// is not open, such as a `</b>` with no `<b>` before it, has no effect on
/// the text and is dropped. Unknown ASS override commands become
/// [`Span::Raw`].
pub fn parse(input: &str, dialect: Format) -> ParsedText {
    match dialect {
        Format::Srt | Format::Vtt => parse_markup(input, dialect),
        Format::Ass => parse_ass(input),
    }
}

/// Renders spans as cue text in `dialect`.
///
/// SRT and WebVTT text never contains an empty line, so a line break that
/// would create one is written as a line holding one space.
pub fn render(spans: &[Span], dialect: Format) -> Rendered {
    let mut output = Rendered::default();
    render_into(spans, dialect, &mut output);
    if dialect != Format::Ass && output.text.ends_with('\n') {
        output.text.push(' ');
    }
    output
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Wrap {
    Bold,
    Italic,
    Underline,
    Strike,
    Color(Rgb),
}

impl Wrap {
    fn into_span(self, children: Vec<Span>) -> Span {
        match self {
            Self::Bold => Span::Bold(children),
            Self::Italic => Span::Italic(children),
            Self::Underline => Span::Underline(children),
            Self::Strike => Span::Strike(children),
            Self::Color(rgb) => Span::Color(rgb, children),
        }
    }

    fn is_color(self) -> bool {
        matches!(self, Self::Color(_))
    }
}

struct Frame {
    wrap: Wrap,
    children: Vec<Span>,
    reopened: bool,
}

#[derive(Default)]
struct Builder {
    root: Vec<Span>,
    open: Vec<Frame>,
    text: String,
}

impl Builder {
    fn target(&mut self) -> &mut Vec<Span> {
        match self.open.last_mut() {
            Some(frame) => &mut frame.children,
            None => &mut self.root,
        }
    }

    fn flush(&mut self) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.target().push(Span::Text(text));
        }
    }

    fn push_text(&mut self, text: &str) {
        for (position, piece) in text.split('\n').enumerate() {
            if position > 0 {
                self.emit(Span::LineBreak);
            }
            self.text.push_str(piece);
        }
    }

    fn emit(&mut self, span: Span) {
        self.flush();
        self.target().push(span);
    }

    fn open(&mut self, wrap: Wrap) {
        self.flush();
        self.open.push(Frame {
            wrap,
            children: Vec::new(),
            reopened: false,
        });
    }

    fn pop_frame(&mut self) -> Option<Wrap> {
        let frame = self.open.pop()?;
        if !(frame.reopened && frame.children.is_empty()) {
            let span = frame.wrap.into_span(frame.children);
            self.target().push(span);
        }
        Some(frame.wrap)
    }

    fn close(&mut self, matches: impl Fn(Wrap) -> bool) {
        let Some(position) = self.open.iter().rposition(|frame| matches(frame.wrap)) else {
            return;
        };
        self.flush();
        let mut reopen = Vec::new();
        while self.open.len() > position + 1 {
            if let Some(wrap) = self.pop_frame() {
                reopen.push(wrap);
            }
        }
        self.pop_frame();
        for wrap in reopen.into_iter().rev() {
            self.open.push(Frame {
                wrap,
                children: Vec::new(),
                reopened: true,
            });
        }
    }

    fn close_all(&mut self) {
        self.flush();
        while self.pop_frame().is_some() {}
    }

    fn finish(mut self) -> Vec<Span> {
        self.close_all();
        self.root
    }
}

fn parse_markup(input: &str, dialect: Format) -> ParsedText {
    let mut builder = Builder::default();
    let mut voice = None;
    let mut rest = input;
    while let Some(open_at) = rest.find('<') {
        builder.push_text(&rest[..open_at]);
        let after = &rest[open_at + 1..];
        let Some(close_at) = after.find('>') else {
            builder.push_text(&rest[open_at..]);
            rest = "";
            break;
        };
        let tag = &after[..close_at];
        if tag.contains('<') {
            builder.push_text("<");
            rest = after;
            continue;
        }
        rest = &after[close_at + 1..];
        markup_tag(&mut builder, tag, dialect, &mut voice);
    }
    builder.push_text(rest);
    ParsedText {
        spans: builder.finish(),
        alignment: None,
        voice,
    }
}

fn markup_tag(builder: &mut Builder, tag: &str, dialect: Format, voice: &mut Option<String>) {
    let (closing, body) = match tag.strip_prefix('/') {
        Some(body) => (true, body),
        None => (false, tag),
    };
    let name_end = body
        .find(|character: char| character.is_whitespace() || character == '.')
        .unwrap_or(body.len());
    let name = body[..name_end].to_ascii_lowercase();
    let wrap = match name.as_str() {
        "b" => Some(Wrap::Bold),
        "i" => Some(Wrap::Italic),
        "u" => Some(Wrap::Underline),
        "s" if dialect == Format::Srt => Some(Wrap::Strike),
        _ => None,
    };
    if let Some(wrap) = wrap {
        if closing {
            builder.close(|open| open == wrap);
        } else {
            builder.open(wrap);
        }
        return;
    }
    if name == "font" {
        if closing {
            builder.close(Wrap::is_color);
        } else if let Some(rgb) = font_colour(body) {
            builder.open(Wrap::Color(rgb));
        } else {
            builder.emit(Span::Raw(tag.to_owned()));
        }
        return;
    }
    if dialect == Format::Vtt {
        match name.as_str() {
            "v" => {
                if !closing && voice.is_none() {
                    *voice = Some(body[name_end..].trim().to_owned());
                }
                return;
            }
            "c" | "lang" | "ruby" | "rt" => {
                if !closing {
                    builder.emit(Span::Raw(tag.to_owned()));
                }
                return;
            }
            _ if is_timestamp_tag(tag) => {
                builder.emit(Span::Raw(tag.to_owned()));
                return;
            }
            _ => {}
        }
    }
    builder.text.push('<');
    builder.text.push_str(tag);
    builder.text.push('>');
}

fn parse_ass(input: &str) -> ParsedText {
    let mut builder = Builder::default();
    let mut alignment = None;
    let mut rest = input;
    while let Some(position) = rest.find(['\\', '{']) {
        builder.push_text(&rest[..position]);
        let tail = &rest[position..];
        if let Some(after) = tail
            .strip_prefix("\\N")
            .or_else(|| tail.strip_prefix("\\n"))
        {
            builder.emit(Span::LineBreak);
            rest = after;
        } else if let Some(after) = tail.strip_prefix("\\h") {
            builder.text.push(' ');
            rest = after;
        } else if let Some(after) = tail.strip_prefix('\\') {
            builder.text.push('\\');
            rest = after;
        } else if let Some(end) = tail.find('}') {
            for command in tail[1..end]
                .split('\\')
                .filter(|command| !command.is_empty())
            {
                override_command(&mut builder, command, &mut alignment);
            }
            rest = &tail[end + 1..];
        } else {
            builder.push_text(tail);
            rest = "";
        }
    }
    builder.push_text(rest);
    ParsedText {
        spans: builder.finish(),
        alignment,
        voice: None,
    }
}

fn override_command(builder: &mut Builder, command: &str, alignment: &mut Option<u8>) {
    let toggle = |wrap: Wrap, value: &str| match value {
        "1" => Some((wrap, true)),
        "0" => Some((wrap, false)),
        _ => None,
    };
    let toggled = match command.as_bytes().first() {
        Some(b'b')
            if command[1..].bytes().all(|byte| byte.is_ascii_digit()) && command.len() > 1 =>
        {
            Some((Wrap::Bold, command != "b0"))
        }
        Some(b'i') => toggle(Wrap::Italic, &command[1..]),
        Some(b'u') => toggle(Wrap::Underline, &command[1..]),
        Some(b's') => toggle(Wrap::Strike, &command[1..]),
        _ => None,
    };
    if let Some((wrap, open)) = toggled {
        if open {
            builder.open(wrap);
        } else {
            builder.close(|candidate| candidate == wrap);
        }
        return;
    }
    if command == "c" || command == "1c" {
        builder.close(Wrap::is_color);
        return;
    }
    if command.starts_with("c&H") || command.starts_with("1c&H") {
        match ass_colour(command) {
            Some(rgb) => {
                builder.close(Wrap::is_color);
                builder.open(Wrap::Color(rgb));
            }
            None => builder.emit(Span::Raw(command.to_owned())),
        }
        return;
    }
    if command.starts_with('r') {
        builder.close_all();
        return;
    }
    if let Some(digit) = command.strip_prefix("an") {
        if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() {
            *alignment = digit.parse().ok();
            return;
        }
    }
    builder.emit(Span::Raw(command.to_owned()));
}

fn render_into(spans: &[Span], dialect: Format, output: &mut Rendered) {
    for span in spans {
        match span {
            Span::Text(text) => output.text.push_str(text),
            Span::LineBreak => {
                if dialect == Format::Ass {
                    output.text.push_str("\\N");
                } else {
                    if output.text.is_empty() || output.text.ends_with('\n') {
                        output.text.push(' ');
                    }
                    output.text.push('\n');
                }
            }
            Span::Bold(children) => wrap(children, dialect, output, "b", "b"),
            Span::Italic(children) => wrap(children, dialect, output, "i", "i"),
            Span::Underline(children) => wrap(children, dialect, output, "u", "u"),
            Span::Strike(children) => {
                if dialect == Format::Vtt {
                    drop_tag(output, "s");
                    render_into(children, dialect, output);
                } else {
                    wrap(children, dialect, output, "s", "s");
                }
            }
            Span::Color(rgb, children) => match dialect {
                Format::Srt => {
                    output.text.push_str(&format!(
                        "<font color=\"#{:02x}{:02x}{:02x}\">",
                        rgb.red, rgb.green, rgb.blue
                    ));
                    render_into(children, dialect, output);
                    output.text.push_str("</font>");
                }
                Format::Vtt => {
                    drop_tag(output, "font");
                    render_into(children, dialect, output);
                }
                Format::Ass => {
                    output.text.push_str(&format!(
                        "{{\\c&H{:02X}{:02X}{:02X}&}}",
                        rgb.blue, rgb.green, rgb.red
                    ));
                    render_into(children, dialect, output);
                    output.text.push_str("{\\c}");
                }
            },
            Span::Raw(tag) => drop_tag(output, tag),
        }
    }
}

fn wrap(children: &[Span], dialect: Format, output: &mut Rendered, html: &str, ass: &str) {
    if dialect == Format::Ass {
        output.text.push_str(&format!("{{\\{ass}1}}"));
        render_into(children, dialect, output);
        output.text.push_str(&format!("{{\\{ass}0}}"));
    } else {
        output.text.push_str(&format!("<{html}>"));
        render_into(children, dialect, output);
        output.text.push_str(&format!("</{html}>"));
    }
}

fn drop_tag(output: &mut Rendered, tag: &str) {
    if !output.dropped.iter().any(|dropped| dropped == tag) {
        output.dropped.push(tag.to_owned());
    }
}

fn font_colour(tag: &str) -> Option<Rgb> {
    let lower = tag.to_ascii_lowercase();
    let position = lower.find("color=")? + 6;
    let value = tag[position..].trim();
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.split_once('"').map(|pair| pair.0))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.split_once('\'').map(|pair| pair.0))
        })
        .unwrap_or_else(|| value.split_ascii_whitespace().next().unwrap_or(""));
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Some(Rgb {
                red: u8::from_str_radix(&hex[0..2], 16).ok()?,
                green: u8::from_str_radix(&hex[2..4], 16).ok()?,
                blue: u8::from_str_radix(&hex[4..6], 16).ok()?,
            });
        }
    }
    let (red, green, blue) = match value.to_ascii_lowercase().as_str() {
        "red" => (255, 0, 0),
        "yellow" => (255, 255, 0),
        "blue" => (0, 0, 255),
        "green" => (0, 128, 0),
        "white" => (255, 255, 255),
        "black" => (0, 0, 0),
        _ => return None,
    };
    Some(Rgb { red, green, blue })
}

fn ass_colour(command: &str) -> Option<Rgb> {
    let hex = command.split_once("&H")?.1.trim_end_matches('&');
    if hex.len() < 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let hex = &hex[hex.len() - 6..];
    Some(Rgb {
        red: u8::from_str_radix(&hex[4..6], 16).ok()?,
        green: u8::from_str_radix(&hex[2..4], 16).ok()?,
        blue: u8::from_str_radix(&hex[0..2], 16).ok()?,
    })
}

fn is_timestamp_tag(tag: &str) -> bool {
    tag.contains(':')
        && tag.contains('.')
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b':' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Span {
        Span::Text(value.to_owned())
    }

    fn reparse(input: &str, dialect: Format) -> (ParsedText, String) {
        let first = parse(input, dialect);
        let rendered = render(&first.spans, dialect).text;
        let second = parse(&rendered, dialect);
        assert_eq!(first.spans, second.spans, "{input:?} in {dialect:?}");
        (first, rendered)
    }

    #[test]
    fn nested_markup_builds_a_tree() {
        let parsed = parse("<b>bold <i>both</i></b> plain", Format::Srt);
        assert_eq!(
            parsed.spans,
            vec![
                Span::Bold(vec![text("bold "), Span::Italic(vec![text("both")])]),
                text(" plain"),
            ]
        );
    }

    #[test]
    fn crossing_tags_close_and_reopen() {
        let parsed = parse("<b><i>x</b>y</i>", Format::Srt);
        assert_eq!(
            parsed.spans,
            vec![
                Span::Bold(vec![Span::Italic(vec![text("x")])]),
                Span::Italic(vec![text("y")]),
            ]
        );
        assert_eq!(
            render(&parsed.spans, Format::Srt).text,
            "<b><i>x</i></b><i>y</i>"
        );
    }

    #[test]
    fn ass_toggles_map_to_the_same_tree() {
        let ass = parse("{\\b1}bold {\\i1}both{\\i0}{\\b0} plain", Format::Ass);
        let srt = parse("<b>bold <i>both</i></b> plain", Format::Srt);
        assert_eq!(ass.spans, srt.spans);
        assert_eq!(
            render(&srt.spans, Format::Ass).text,
            "{\\b1}bold {\\i1}both{\\i0}{\\b0} plain"
        );
    }

    #[test]
    fn colour_round_trips_between_srt_and_ass() {
        let srt = parse("<font color=\"#ff8000\">warm</font>", Format::Srt);
        let ass_text = render(&srt.spans, Format::Ass).text;
        assert_eq!(ass_text, "{\\c&H0080FF&}warm{\\c}");
        let ass = parse(&ass_text, Format::Ass);
        assert_eq!(ass.spans, srt.spans);
        assert_eq!(
            render(&ass.spans, Format::Srt).text,
            "<font color=\"#ff8000\">warm</font>"
        );
    }

    #[test]
    fn ass_reset_closes_every_open_span() {
        let parsed = parse("{\\b1\\i1}x{\\r}y", Format::Ass);
        assert_eq!(
            parsed.spans,
            vec![Span::Bold(vec![Span::Italic(vec![text("x")])]), text("y")]
        );
    }

    #[test]
    fn ass_breaks_spaces_and_alignment() {
        let parsed = parse("{\\an8}top\\Nline\\hspaced", Format::Ass);
        assert_eq!(parsed.alignment, Some(8));
        assert_eq!(
            parsed.spans,
            vec![text("top"), Span::LineBreak, text("line spaced")]
        );
        assert_eq!(render(&parsed.spans, Format::Srt).text, "top\nline spaced");
    }

    #[test]
    fn unknown_ass_commands_become_raw_and_are_reported() {
        let parsed = parse("{\\pos(10,20)\\blur2}x", Format::Ass);
        assert_eq!(
            parsed.spans,
            vec![
                Span::Raw("pos(10,20)".to_owned()),
                Span::Raw("blur2".to_owned()),
                text("x")
            ]
        );
        let rendered = render(&parsed.spans, Format::Srt);
        assert_eq!(rendered.text, "x");
        assert_eq!(rendered.dropped, vec!["pos(10,20)", "blur2"]);
    }

    #[test]
    fn vtt_only_tags_are_unwrapped_and_reported_once() {
        let parsed = parse("<v Bob><c.yellow>hi</c> <c.yellow>again</c>", Format::Vtt);
        assert_eq!(parsed.voice.as_deref(), Some("Bob"));
        let rendered = render(&parsed.spans, Format::Srt);
        assert_eq!(rendered.text, "hi again");
        assert_eq!(rendered.dropped, vec!["c.yellow"]);
    }

    #[test]
    fn a_bare_less_than_does_not_swallow_the_next_tag() {
        let parsed = parse("fish & chips < 3 <b>ok</b> &amp;", Format::Srt);
        assert_eq!(
            parsed.spans,
            vec![
                text("fish & chips < 3 "),
                Span::Bold(vec![text("ok")]),
                text(" &amp;")
            ]
        );
        let parsed = parse("I <3 you <i>so</i> much", Format::Srt);
        assert_eq!(
            parsed.spans,
            vec![
                text("I <3 you "),
                Span::Italic(vec![text("so")]),
                text(" much")
            ]
        );
        assert_eq!(
            parse("no open</b> here <b>on", Format::Srt).spans,
            vec![text("no open here "), Span::Bold(vec![text("on")])]
        );
    }

    #[test]
    fn unknown_markup_stays_literal() {
        let parsed = parse("a <foo> b <", Format::Srt);
        assert_eq!(parsed.spans, vec![text("a <foo> b <")]);
        assert_eq!(render(&parsed.spans, Format::Vtt).text, "a <foo> b <");
    }

    #[test]
    fn strike_and_colour_drop_in_vtt_but_keep_their_text() {
        let parsed = parse("<s>gone</s> <font color=\"red\">red</font>", Format::Srt);
        let rendered = render(&parsed.spans, Format::Vtt);
        assert_eq!(rendered.text, "gone red");
        assert_eq!(rendered.dropped, vec!["s", "font"]);
    }

    #[test]
    fn empty_lines_are_never_rendered_for_srt_or_vtt() {
        let parsed = parse("\\N\\Na\\N\\Nb\\N", Format::Ass);
        assert_eq!(render(&parsed.spans, Format::Srt).text, " \n \na\n \nb\n ");
        assert_eq!(render(&parsed.spans, Format::Ass).text, "\\N\\Na\\N\\Nb\\N");
    }

    #[test]
    fn rendering_is_a_fixed_point_in_every_dialect() {
        let cases = [
            (Format::Srt, "<b>a</b> <i>b <u>c</u></i> <s>d</s> plain"),
            (
                Format::Srt,
                "<font color=\"#123456\">x</font> <b><i>y</b>z</i>",
            ),
            (Format::Srt, "line one\nline two"),
            (Format::Vtt, "<b>a</b> <i>b <u>c</u></i> <s>d</s>"),
            (
                Format::Ass,
                "{\\b1}a{\\b0} {\\i1}b{\\i0}\\N{\\c&H00FF00&}c{\\c}",
            ),
            (Format::Ass, "{\\an5}d\\Ne {\\u1}f{\\u0}"),
        ];
        for (dialect, input) in cases {
            let (_, rendered) = reparse(input, dialect);
            let again = render(&parse(&rendered, dialect).spans, dialect).text;
            assert_eq!(rendered, again, "{input:?}");
        }
    }

    #[test]
    fn cross_dialect_round_trip_preserves_the_tree() {
        let sources = [
            "<b>a <i>b</i></b> <u>c</u> <s>d</s>\n<font color=\"blue\">e</font>",
            "<b><i>x</b>y</i>",
        ];
        for source in sources {
            let original = parse(source, Format::Srt).spans;
            let ass = render(&original, Format::Ass).text;
            let back = parse(&ass, Format::Ass).spans;
            assert_eq!(back, original, "{source:?}");
            let srt = render(&back, Format::Srt).text;
            assert_eq!(parse(&srt, Format::Srt).spans, original, "{source:?}");
        }
    }
}

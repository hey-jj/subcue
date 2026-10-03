# subcue

`subcue` parses, composes, and converts SRT, WebVTT, and ASS/SSA subtitle files.
It accepts byte slices for complete files and `BufRead` values for cue iteration.

The data model keeps cue order, signed millisecond timestamps, raw cue text, and
format metadata. Parsing accepts UTF-8. A UTF-8 byte order mark is removed with a
warning. UTF-16 input and invalid UTF-8 return typed errors.

## Parse and compose

```rust
use subcue::{parse_srt, Time};

let input = b"1\n00:00:01,000 --> 00:00:02,500\nHello\n\n";
let subtitles = parse_srt(input)?;

assert_eq!(subtitles.cues.len(), 1);
assert_eq!(subtitles.cues[0].start, Time(1_000));
assert_eq!(subtitles.cues[0].text, "Hello");

let (bytes, warnings) = subtitles.compose();
assert_eq!(bytes, input);
assert!(warnings.is_empty());
# Ok::<(), subcue::Error>(())
```

`compose_with` can write CRLF line endings or add a UTF-8 byte order mark.
Default composition writes LF line endings without a mark.

## Convert formats

```rust
use subcue::{parse_srt, Format};

let input = b"1\n00:00:01,000 --> 00:00:02,000\n<i>Hello</i>\n\n";
let srt = parse_srt(input)?;
let vtt = srt.convert(Format::Vtt);
let (bytes, warnings) = vtt.compose();

assert!(bytes.starts_with(b"WEBVTT\n\n"));
assert!(warnings.is_empty());
# Ok::<(), subcue::Error>(())
```

Conversion keeps cue order and timing. ASS output truncates milliseconds to
centiseconds and reports `Warning::TimePrecisionLost`. Settings, identifiers,
blocks, events, and text tags that have no target representation produce a
specific warning.

Every conversion reads the source tags into a `text::Span` tree and writes
that tree in the target dialect. The `text` module is public, so a caller can
parse or render inline formatting without converting a whole file.

```rust
use subcue::text::{parse, render, Span};
use subcue::Format;

let parsed = parse("<b>bold <i>both</i></b>", Format::Srt);
assert!(matches!(parsed.spans[0], Span::Bold(_)));
let ass = render(&parsed.spans, Format::Ass);
assert_eq!(ass.text, "{\\b1}bold {\\i1}both{\\i0}{\\b0}");
```

## Iterate over cues

```rust
use std::io::Cursor;
use subcue::SrtCues;

let input = b"1\n00:00:01,000 --> 00:00:02,000\nFirst\n\n";
let mut cues = SrtCues::new(Cursor::new(input));
assert_eq!(cues.next().unwrap()?.text, "First");
assert!(cues.next().is_none());
# Ok::<(), subcue::Error>(())
```

`VttCues` reads the signature and header in its constructor. `AssCues` reads
script and style metadata there. Each iterator keeps one line buffer and one
text accumulator, allocates 1 time per cue for SRT, 3 for WebVTT, and 5 for ASS, and exposes its collected
warnings. A failed read from the source is returned as `Error::Io`.

## Errors and warnings

Fatal errors cover decoding failures, timestamp failures, a missing WebVTT
signature, and content before an ASS section. Recoverable input stays in the
model and adds a warning. Examples include mixed line endings, missing SRT
indices, unknown ASS styles, and a cue that ends before it starts.

The library uses checked integer arithmetic for every timestamp calculation.
All parser entry points return a result for arbitrary input bytes.

## Toolchain and dependencies

The library has no runtime dependencies. It supports Rust 1.70.0 and edition
2021.

## Relation to subparse

subcue is an independent implementation. It is not affiliated with subparse
or its author. Both read SRT, WebVTT, and ASS/SSA files. subcue also composes
each format, converts between the three, and streams cues from a reader.

## License

MIT


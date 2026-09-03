//! Public model types support exact equality and use Rust field names.

use std::collections::HashSet;
use std::error::Error as _;
use std::io::{self, BufRead, Cursor, Read};

use subcue::{
    AssCues, AssEvent, AssFields, Cue, Error, Header, RawSection, SrtCues, Style, Subtitles,
    VttBlock, VttCues,
};

fn requires_eq<T: Eq>() {}

#[test]
fn owned_model_types_implement_eq() {
    requires_eq::<Subtitles>();
    requires_eq::<Cue>();
    requires_eq::<AssFields>();
    requires_eq::<AssEvent>();
    requires_eq::<Header>();
    requires_eq::<VttBlock>();
    requires_eq::<RawSection>();
    requires_eq::<Style>();
}

#[test]
fn style_font_fields_use_rust_names() {
    let style = Style {
        font_name: "Noto Sans".to_owned(),
        ..Style::default()
    };
    assert_eq!(style.font_name, "Noto Sans");
    assert_eq!(style.font_size.to_string(), "20");
    let mut seen = HashSet::new();
    assert!(seen.insert(style.font_name.clone()));
}

struct FailingReader;

impl Read for FailingReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
    }
}

impl BufRead for FailingReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"))
    }

    fn consume(&mut self, _: usize) {}
}

fn expect_io(error: Error) {
    match &error {
        Error::Io(inner) => assert_eq!(inner.kind(), io::ErrorKind::BrokenPipe),
        other => panic!("expected Io, got {other:?}"),
    }
    assert!(error.source().is_some());
    assert_eq!(error.to_string(), "read failed: pipe closed");
}

#[test]
fn reader_failures_are_io_errors_not_decoding_errors() {
    expect_io(SrtCues::new(FailingReader).next().unwrap().unwrap_err());
    expect_io(VttCues::new(FailingReader).err().unwrap());
    expect_io(AssCues::new(FailingReader).err().unwrap());
    let converted: Error = io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed").into();
    expect_io(converted);
}

#[test]
fn a_reader_that_fails_mid_stream_yields_the_cues_before_the_failure() {
    let good = b"1\n00:00:01,000 --> 00:00:02,000\nfirst\n\n";
    let reader = Cursor::new(&good[..]).chain(FailingReader);
    let mut cues = SrtCues::new(io::BufReader::new(reader));
    assert_eq!(cues.next().unwrap().unwrap().text, "first");
    assert!(matches!(cues.next(), Some(Err(Error::Io(_)))));
    assert!(cues.next().is_none());
}

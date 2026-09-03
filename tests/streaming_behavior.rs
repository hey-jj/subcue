use std::cell::Cell;
use std::io::{self, BufRead, Cursor, Read};
use std::rc::Rc;

use subcue::{AssCues, Error, SrtCues, VttCues};

#[test]
fn ass_stream_reports_a_bad_dialogue_and_continues() {
    let input = b"[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,first\nDialogue: 0,0:00:xx.00,0:00:03.00,Default,,0,0,0,,bad\nDialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,third\n";
    let mut cues = AssCues::new(Cursor::new(input)).unwrap();
    assert_eq!(cues.next().unwrap().unwrap().text, "first");
    assert!(matches!(
        cues.next().unwrap(),
        Err(Error::InvalidTimestamp { .. })
    ));
    assert_eq!(cues.next().unwrap().unwrap().text, "third");
    assert!(cues.next().is_none());
}

#[test]
fn streaming_constructors_do_not_consume_cue_bodies() {
    let srt =
        b"1\n00:00:01,000 --> 00:00:02,000\nfirst\n\n2\n00:00:03,000 --> 00:00:04,000\nsecond\n\n";
    let (reader, consumed) = CountingReader::new(Cursor::new(srt));
    let _cues = SrtCues::new(reader);
    assert_eq!(consumed.get(), 0);

    let vtt = b"WEBVTT\nKind: captions\n\n00:00:01.000 --> 00:00:02.000\nfirst\n\n00:00:03.000 --> 00:00:04.000\nsecond\n\n";
    let (reader, consumed) = CountingReader::new(Cursor::new(vtt));
    let _cues = VttCues::new(reader).unwrap();
    assert!(consumed.get() < vtt.len());

    let ass = b"[Script Info]\nTitle: bounded\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,first\nDialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,second\n";
    let (reader, consumed) = CountingReader::new(Cursor::new(ass));
    let _cues = AssCues::new(reader).unwrap();
    assert!(consumed.get() < ass.len());
}

struct CountingReader<R> {
    inner: R,
    consumed: Rc<Cell<usize>>,
}

impl<R> CountingReader<R> {
    fn new(inner: R) -> (Self, Rc<Cell<usize>>) {
        let consumed = Rc::new(Cell::new(0));
        (
            Self {
                inner,
                consumed: Rc::clone(&consumed),
            },
            consumed,
        )
    }
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl<R: BufRead> BufRead for CountingReader<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.consumed.set(self.consumed.get() + amount);
        self.inner.consume(amount);
    }
}

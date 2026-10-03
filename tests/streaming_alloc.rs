//! Allocation gate for the streaming iterators.
//!
//! Each cue costs at most 1 allocation for SRT, 3 for WebVTT, and 5 for ASS,
//! plus a per-file constant. Counts do not grow with lines per cue. The counter is
//! thread local, so allocations made by the test harness on other threads do
//! not leak into a measurement.

mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::io::Cursor;

use subcue::{AssCues, SrtCues, VttCues};

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count_one() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_one();
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout)
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_one();
        System.realloc(pointer, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn measured(run: impl FnOnce() -> usize) -> (usize, usize) {
    let before = ALLOCATIONS.with(Cell::get);
    let yielded = run();
    (yielded, ALLOCATIONS.with(Cell::get) - before)
}

type Stream = fn(&[u8]) -> usize;
type Generate = fn(usize, usize) -> Vec<u8>;

const PER_CUE_SRT: usize = 1;
const PER_CUE_VTT: usize = 3;
const PER_CUE_ASS: usize = 5;
const PER_FILE: usize = 96;

fn timing(index: usize, separator: char) -> String {
    let start = index as i64 * 3000;
    let end = start + 2500;
    let clock = |millis: i64| {
        format!(
            "{:02}:{:02}:{:02}{separator}{:03}",
            millis / 3_600_000,
            millis / 60_000 % 60,
            millis / 1000 % 60,
            millis % 1000
        )
    };
    format!("{} --> {}", clock(start), clock(end))
}

fn srt_input(cues: usize, lines_per_cue: usize) -> Vec<u8> {
    let mut output = String::new();
    for index in 0..cues {
        output.push_str(&format!("{}\n{}\n", index + 1, timing(index, ',')));
        for line in 0..lines_per_cue {
            output.push_str(&format!("<i>line {line}</i> of cue {}\n", index + 1));
        }
        output.push('\n');
    }
    output.into_bytes()
}

fn vtt_input(cues: usize, lines_per_cue: usize) -> Vec<u8> {
    let mut output = String::from("WEBVTT\n\n");
    for index in 0..cues {
        output.push_str(&format!(
            "{}\n{} align:start\n",
            index + 1,
            timing(index, '.')
        ));
        for line in 0..lines_per_cue {
            output.push_str(&format!("<i>line {line}</i> of cue {}\n", index + 1));
        }
        output.push('\n');
    }
    output.into_bytes()
}

fn ass_input(cues: usize, lines_per_cue: usize) -> Vec<u8> {
    let mut output = String::from(
        "[Script Info]\nTitle: alloc\nScriptType: v4.00+\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
    for index in 0..cues {
        let start = index as i64 * 3000;
        let end = start + 2500;
        let clock = |millis: i64| {
            format!(
                "{}:{:02}:{:02}.{:02}",
                millis / 3_600_000,
                millis / 60_000 % 60,
                millis / 1000 % 60,
                millis % 1000 / 10
            )
        };
        output.push_str(&format!(
            "Dialogue: 0,{},{},Default,,0,0,0,,",
            clock(start),
            clock(end)
        ));
        for line in 0..lines_per_cue {
            if line > 0 {
                output.push_str("\\N");
            }
            output.push_str(&format!("{{\\i1}}line {line}{{\\i0}} of cue {}", index + 1));
        }
        output.push('\n');
    }
    output.into_bytes()
}

fn stream_srt(input: &[u8]) -> usize {
    SrtCues::new(Cursor::new(input)).fold(0, |count, cue| {
        cue.unwrap();
        count + 1
    })
}

fn stream_vtt(input: &[u8]) -> usize {
    VttCues::new(Cursor::new(input))
        .unwrap()
        .fold(0, |count, cue| {
            cue.unwrap();
            count + 1
        })
}

fn stream_ass(input: &[u8]) -> usize {
    AssCues::new(Cursor::new(input))
        .unwrap()
        .fold(0, |count, cue| {
            cue.unwrap();
            count + 1
        })
}

#[test]
fn streaming_allocations_are_bounded_per_cue_on_the_perf_shapes() {
    let count = 500;
    let cases: [(&str, Vec<u8>, Stream, usize); 3] = [
        ("srt", support::perf_srt(count), stream_srt, PER_CUE_SRT),
        ("vtt", support::perf_vtt(count), stream_vtt, PER_CUE_VTT),
        ("ass", support::perf_ass(count), stream_ass, PER_CUE_ASS),
    ];
    for (name, input, stream, per_cue) in cases {
        let (yielded, allocations) = measured(|| stream(&input));
        assert_eq!(yielded, count, "{name}");
        assert!(
            allocations <= count * per_cue + PER_FILE,
            "{name} used {allocations} allocations for {count} cues"
        );
    }
}

#[test]
fn streaming_allocations_do_not_grow_with_lines_per_cue() {
    let count = 400;
    let cases: [(&str, Generate, Stream, usize); 3] = [
        ("srt", srt_input, stream_srt, PER_CUE_SRT),
        ("vtt", vtt_input, stream_vtt, PER_CUE_VTT),
        ("ass", ass_input, stream_ass, PER_CUE_ASS),
    ];
    for (name, generate, stream, per_cue) in cases {
        let mut counts = Vec::new();
        for lines_per_cue in [1, 4, 16] {
            let input = generate(count, lines_per_cue);
            let (yielded, allocations) = measured(|| stream(&input));
            assert_eq!(yielded, count, "{name} with {lines_per_cue} lines");
            assert!(
                allocations <= count * per_cue + PER_FILE,
                "{name} with {lines_per_cue} lines used {allocations} allocations"
            );
            counts.push(allocations);
        }
        let spread = counts.iter().max().unwrap() - counts.iter().min().unwrap();
        assert!(
            spread <= 32,
            "{name} allocation counts {counts:?} grow with lines per cue"
        );
    }
}

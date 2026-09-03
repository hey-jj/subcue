mod support;

#[cfg(not(debug_assertions))]
use std::time::{Duration, Instant};

#[cfg(not(debug_assertions))]
use subcue::{parse_ass, parse_srt, parse_vtt, Format};

#[test]
fn generated_performance_inputs_match_the_pins() {
    assert_eq!(
        support::sha256(&support::perf_srt(10_000)),
        "bd8e54bd019c131372911bf38a337cca4b1b1b85ff9fba635bf41f5a5f87a133"
    );
    assert_eq!(
        support::sha256(&support::perf_vtt(10_000)),
        "10f00b4cea575d24c17299231b064b217b8ebe27672bd29f7f8b5d3e8ee4db90"
    );
    assert_eq!(
        support::sha256(&support::perf_ass(10_000)),
        "4d78865d808971d002cf61d63c87261251c938e30c6d14c9759df61fbfd9155a"
    );
}

#[test]
#[cfg(not(debug_assertions))]
fn release_parse_and_compose_stay_under_the_floor() {
    let cases = [
        (support::perf_srt(10_000), subcue::Format::Srt),
        (support::perf_vtt(10_000), subcue::Format::Vtt),
        (support::perf_ass(10_000), subcue::Format::Ass),
    ];
    for (input, format) in cases {
        let started = Instant::now();
        let subtitles = match format {
            subcue::Format::Srt => parse_srt(&input).unwrap(),
            subcue::Format::Vtt => parse_vtt(&input).unwrap(),
            subcue::Format::Ass => parse_ass(&input).unwrap(),
        };
        let parse_elapsed = started.elapsed();
        let started = Instant::now();
        let _ = subtitles.compose();
        let compose_elapsed = started.elapsed();
        assert!(
            parse_elapsed < Duration::from_millis(100),
            "{format:?} parse took {parse_elapsed:?}"
        );
        assert!(
            compose_elapsed < Duration::from_millis(100),
            "{format:?} compose took {compose_elapsed:?}"
        );
    }
}

#[test]
#[cfg(not(debug_assertions))]
fn release_parse_growth_is_linear() {
    let ten = support::perf_srt(10_000);
    let twenty = support::perf_srt(20_000);
    let ten_elapsed = measured_parse(&ten);
    let twenty_elapsed = measured_parse(&twenty);
    assert!(
        twenty_elapsed.as_nanos() <= ten_elapsed.as_nanos() * 3,
        "20k took {twenty_elapsed:?}; 10k took {ten_elapsed:?}"
    );
}

#[cfg(not(debug_assertions))]
fn measured_parse(input: &[u8]) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let started = Instant::now();
        let parsed = parse_srt(input).unwrap();
        std::hint::black_box(parsed);
        best = best.min(started.elapsed());
    }
    best
}

#[cfg(not(debug_assertions))]
fn warned_srt(count: usize) -> Vec<u8> {
    let mut output = String::new();
    for index in 0..count {
        let start = index as i64 * 3000 + 5;
        output.push_str(&format!(
            "{}\n00:{:02}:{:02},{:03} --> 00:{:02}:{:02},{:03}\n<b>x</b> <s>y</s>\n\n",
            index + 1,
            start / 60_000 % 60,
            start / 1000 % 60,
            start % 1000,
            (start + 1000) / 60_000 % 60,
            (start + 1000) / 1000 % 60,
            (start + 1000) % 1000
        ));
    }
    output.into_bytes()
}

#[cfg(not(debug_assertions))]
fn styled_ass(count: usize) -> Vec<u8> {
    let mut output = String::from("[V4+ Styles]\nFormat: Name, Fontname\n");
    for index in 0..count {
        output.push_str(&format!("Style: s{index},Arial\n"));
    }
    output.push_str("\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    for index in 0..count {
        output.push_str(&format!(
            "Dialogue: 0,0:00:01.00,0:00:02.00,s{},,0,0,0,,x\n",
            count - 1 - index
        ));
    }
    output.into_bytes()
}

/// Conversion, compose of a converted model, and the unknown-style check
/// all emit or scan one entry per cue. Doubling the input must not
/// quadruple the time.
#[test]
#[cfg(not(debug_assertions))]
fn release_convert_compose_and_style_checks_grow_linearly() {
    let convert_and_compose = |input: &[u8]| {
        let started = Instant::now();
        let converted = parse_srt(input).unwrap().convert(Format::Ass);
        let composed = converted.compose();
        std::hint::black_box(composed);
        started.elapsed()
    };
    let small = convert_and_compose(&warned_srt(20_000));
    let large = convert_and_compose(&warned_srt(40_000));
    assert!(
        large.as_nanos() <= small.as_nanos() * 3,
        "convert+compose: 40k took {large:?}; 20k took {small:?}"
    );
    let style_check = |input: &[u8]| {
        let started = Instant::now();
        std::hint::black_box(parse_ass(input).unwrap());
        started.elapsed()
    };
    let small = style_check(&styled_ass(20_000));
    let large = style_check(&styled_ass(40_000));
    assert!(
        large.as_nanos() <= small.as_nanos() * 3,
        "style check: 40k took {large:?}; 20k took {small:?}"
    );
}

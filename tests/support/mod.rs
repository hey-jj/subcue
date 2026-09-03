#![allow(dead_code)]

use subcue::{Error, Warning};

pub fn warning_name(warning: &Warning) -> &'static str {
    match warning {
        Warning::BomStripped => "BomStripped",
        Warning::MixedLineEndings => "MixedLineEndings",
        Warning::NoCues => "NoCues",
        Warning::ComponentOutOfRange { .. } => "ComponentOutOfRange",
        Warning::EndBeforeStart { .. } => "EndBeforeStart",
        Warning::MissingIndex { .. } => "MissingIndex",
        Warning::NonSequentialIndex { .. } => "NonSequentialIndex",
        Warning::StrayText { .. } => "StrayText",
        Warning::EmptyCue { .. } => "EmptyCue",
        Warning::DotMillis { .. } => "DotMillis",
        Warning::ShortMillis { .. } => "ShortMillis",
        Warning::MissingHours { .. } => "MissingHours",
        Warning::MissingBlankLine { .. } => "MissingBlankLine",
        Warning::UnknownTimingSuffix { .. } => "UnknownTimingSuffix",
        Warning::CommaMillis { .. } => "CommaMillis",
        Warning::ShortHours { .. } => "ShortHours",
        Warning::UnknownSection { .. } => "UnknownSection",
        Warning::FieldCountMismatch { .. } => "FieldCountMismatch",
        Warning::MissingFormatLine { .. } => "MissingFormatLine",
        Warning::AssFractionDigits { .. } => "AssFractionDigits",
        Warning::UnknownStyle { .. } => "UnknownStyle",
        Warning::BadField { .. } => "BadField",
        Warning::UpgradedToV4Plus => "UpgradedToV4Plus",
        Warning::MissingStyles => "MissingStyles",
        Warning::FormatReordered => "FormatReordered",
        Warning::TimePrecisionLost { .. } => "TimePrecisionLost",
        Warning::LossyTag { .. } => "LossyTag",
        Warning::DroppedSettings { .. } => "DroppedSettings",
        Warning::DroppedId { .. } => "DroppedId",
        Warning::DroppedBlock { .. } => "DroppedBlock",
        Warning::DroppedEvent { .. } => "DroppedEvent",
    }
}

pub fn error_name(error: &Error) -> &'static str {
    match error {
        Error::Utf16Input => "Utf16Input",
        Error::InvalidUtf8 { .. } => "InvalidUtf8",
        Error::TimeOutOfRange { .. } => "TimeOutOfRange",
        Error::NegativeTime { .. } => "NegativeTime",
        Error::InvalidTimestamp { .. } => "InvalidTimestamp",
        Error::MissingArrow { .. } => "MissingArrow",
        Error::MissingHeader { .. } => "MissingHeader",
        Error::NotAss { .. } => "NotAss",
        Error::Io(_) => "Io",
    }
}

pub fn error_line(error: &Error) -> Option<usize> {
    match error {
        Error::TimeOutOfRange { line }
        | Error::NegativeTime { line }
        | Error::InvalidTimestamp { line }
        | Error::MissingArrow { line }
        | Error::NotAss { line }
        | Error::MissingHeader { line, .. } => Some(*line),
        Error::Utf16Input | Error::InvalidUtf8 { .. } | Error::Io(_) => None,
    }
}

pub fn error_offset(error: &Error) -> Option<usize> {
    match error {
        Error::InvalidUtf8 { offset } => Some(*offset),
        _ => None,
    }
}

pub fn sha256(input: &[u8]) -> String {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let bit_len = (input.len() as u64) * 8;
    let mut data = input.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    let mut hash = INITIAL;
    for chunk in data.chunks_exact(64) {
        let mut words = [0_u32; 64];
        for (index, word) in words[..16].iter_mut().enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;
        for index in 0..64 {
            let upper = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let first = h
                .wrapping_add(upper)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let lower = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let second = lower.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(first);
            d = c;
            c = b;
            b = a;
            a = first.wrapping_add(second);
        }
        for (state, value) in hash.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *state = state.wrapping_add(value);
        }
    }
    hash.iter().map(|word| format!("{word:08x}")).collect()
}

pub fn perf_srt(count: usize) -> Vec<u8> {
    let mut output = String::new();
    for index in 0..count {
        let start = index as i64 * 3000;
        let end = start + 2500;
        output.push_str(&format!(
            "{}\n{:02}:{:02}:{:02},{:03} --> {:02}:{:02}:{:02},{:03}\nLine one of cue {}\n<i>Line two</i> of cue {}\n\n",
            index + 1,
            start / 3_600_000,
            start / 60_000 % 60,
            start / 1000 % 60,
            start % 1000,
            end / 3_600_000,
            end / 60_000 % 60,
            end / 1000 % 60,
            end % 1000,
            index + 1,
            index + 1
        ));
    }
    output.into_bytes()
}

pub fn perf_vtt(count: usize) -> Vec<u8> {
    let mut output = String::from("WEBVTT\n\n");
    for index in 0..count {
        let start = index as i64 * 3000;
        let end = start + 2500;
        output.push_str(&format!(
            "{}\n{:02}:{:02}:{:02}.{:03} --> {:02}:{:02}:{:02}.{:03} align:start\nLine one of cue {}\n<i>Line two</i> of cue {}\n\n",
            index + 1,
            start / 3_600_000,
            start / 60_000 % 60,
            start / 1000 % 60,
            start % 1000,
            end / 3_600_000,
            end / 60_000 % 60,
            end / 1000 % 60,
            end % 1000,
            index + 1,
            index + 1
        ));
    }
    output.into_bytes()
}

pub fn perf_ass(count: usize) -> Vec<u8> {
    let mut output = String::from("[Script Info]\n; Script generated");
    output.push_str(" by hand\nTitle: Basic\nScriptType: v4.00+\nWrapStyle: 0\nPlayResX: 1280\nPlayResY: 720\nScaledBorderAndShadow: yes\nYCbCr Matrix: TV.709\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1\nStyle: Top,Arial,40,&H00FFFF00,&H000000FF,&H00000000,&H80000000,-1,0,0,0,100,100,0,0,1,2,1,8,10,10,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");
    for index in 0..count {
        let start = index as i64 * 3000;
        let end = start + 2500;
        output.push_str(&format!(
            "Dialogue: 0,{}:{:02}:{:02}.{:02},{}:{:02}:{:02}.{:02},Default,,0,0,0,,Line one of cue {}\\N{{\\i1}}Line two{{\\i0}} of cue {}\n",
            start / 3_600_000,
            start / 60_000 % 60,
            start / 1000 % 60,
            start % 1000 / 10,
            end / 3_600_000,
            end / 60_000 % 60,
            end / 1000 % 60,
            end % 1000 / 10,
            index + 1,
            index + 1
        ));
    }
    output.into_bytes()
}

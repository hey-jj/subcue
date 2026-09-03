#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Parse, compose, and convert SRT, WebVTT, and ASS/SSA subtitle data.

mod compose;
mod convert;
mod model;
mod parse;
mod streaming;
pub mod text;

pub use model::{
    AssEvent, AssFields, AssNumber, Colour, ComposeOptions, Cue, Error, EventKind, Format, Header,
    LineEnding, RawSection, Style, Subtitles, Time, VttBlock, VttBlockKind, Warning,
};
pub use streaming::{AssCues, SrtCues, VttCues};

/// Parses subtitle bytes in the selected format.
pub fn parse(input: &[u8], format: Format) -> Result<Subtitles, Error> {
    match format {
        Format::Srt => parse_srt(input),
        Format::Vtt => parse_vtt(input),
        Format::Ass => parse_ass(input),
    }
}

/// Parses SRT subtitle bytes.
pub fn parse_srt(input: &[u8]) -> Result<Subtitles, Error> {
    parse::parse_srt(input)
}

/// Parses WebVTT subtitle bytes.
pub fn parse_vtt(input: &[u8]) -> Result<Subtitles, Error> {
    parse::parse_vtt(input)
}

/// Parses ASS or SSA subtitle bytes.
pub fn parse_ass(input: &[u8]) -> Result<Subtitles, Error> {
    parse::parse_ass(input)
}

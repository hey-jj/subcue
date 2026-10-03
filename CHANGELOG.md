# Changelog

## 0.1.1 - 2026-10-03

- Limit parsed span nesting to 64 levels to prevent stack overflow during
  conversion. Deeper markup tags stay literal text.
- Enforce streaming allocation bounds of 1 per SRT cue, 3 per WebVTT cue,
  and 5 per ASS cue, and state those bounds in README.
- Correct the SRT45 byte offset and ASS40, ASS44, and ASS45 line numbers
  in conformance-vectors.json.

## 0.1.0 - 2026-09-02

### Added

- Parse and compose SRT, WebVTT, and ASS/SSA files.
- Convert cues between the three supported formats with loss warnings.
- Iterate over cues from `BufRead` inputs.
- Preserve WebVTT blocks, ASS styles, raw ASS sections, and non-dialogue events.
- Return typed decoding and timestamp errors without panicking on input bytes.
- Report error positions as physical byte offsets and one-based physical
  lines after byte order mark removal.
- Report a failed read from a streaming source as `Error::Io`.
- Model inline cue text as `text::Span` and route every conversion through it.
- Ship the conformance records at the crate root so the packaged tests replay
  every fixture byte.
- Keep an SRT text line that holds `-->` as text unless it parses as a timing
  line, and treat a line as a missing-arrow error only when it reads as a time.
- Discard a WebVTT block with no timing line in its first two lines with
  `Warning::StrayText`.
- Write raw ASS sections back at their recorded position, keep non-dialogue
  events and WebVTT blocks after cue removal, and merge duplicate sections.

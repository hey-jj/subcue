use proptest::prelude::*;
use subcue::{parse_ass, parse_srt, parse_vtt, Format};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_never_panic(input in proptest::collection::vec(any::<u8>(), 0..4096)) {
        for subtitles in [parse_srt(&input), parse_vtt(&input), parse_ass(&input)]
            .into_iter()
            .flatten()
        {
            let _ = subtitles.compose();
            for target in [Format::Srt, Format::Vtt, Format::Ass] {
                let _ = subtitles.convert(target).compose();
            }
        }
    }
}

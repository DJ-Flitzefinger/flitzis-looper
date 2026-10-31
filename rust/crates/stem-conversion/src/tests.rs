use super::*;

#[test]
fn endpoints_nonfinite_and_positive_negative_ties_are_exact() {
    assert_eq!(pcm16(-1.0), -32768);
    assert_eq!(pcm16(1.0), 32767);
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(pcm16(value), 0);
    }
    for (value, expected) in [(0.5, 0), (1.5, 2), (2.5, 2), (-0.5, 0), (-1.5, -2)] {
        assert_eq!(pcm16(value / 32767.0), expected);
    }
}

fn source(pcm: &[u8]) -> Source<'_> {
    Source {
        shape: Shape {
            rate: 48000,
            channels: 1,
            frames: 2,
        },
        first: 0,
        pcm,
    }
}

#[test]
fn interpolation_precedes_requantization_and_instrumental_summation() {
    let input = [1_i16, 2].map(i16::to_le_bytes).concat();
    let target = Shape {
        rate: 96000,
        channels: 1,
        frames: 2,
    };
    let output = convert([source(&input); 4], target, 0, 2, 1024, || false).unwrap();
    assert_eq!(output[1], [1_i16, 2].map(i16::to_le_bytes).concat());
    assert_eq!(output[4], [3_i16, 4].map(i16::to_le_bytes).concat());
}

#[test]
fn rejects_span_budget_geometry_and_cancel_before_result() {
    let input = [0; 4];
    let target = source(&input).shape;
    assert_eq!(
        convert([source(&input); 4], target, 0, 2, 1, || false),
        Err(Error::Budget)
    );
    assert_eq!(
        convert([source(&input[..2]); 4], target, 0, 2, 1024, || false),
        Err(Error::Span)
    );
    assert_eq!(
        convert([source(&input); 4], target, 0, 4097, 1024, || false),
        Err(Error::Shape)
    );
    let calls = std::cell::Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() >= 3
    };
    assert_eq!(
        convert([source(&input); 4], target, 0, 2, 1024, cancel),
        Err(Error::Cancelled)
    );
}

#[test]
fn ordered_intermediate_clipping_preserves_the_legacy_sum() {
    let positive = [32767_i16, 32767].map(i16::to_le_bytes).concat();
    let negative = [-32768_i16, -32768].map(i16::to_le_bytes).concat();
    let output = convert(
        [
            source(&positive),
            source(&positive),
            source(&positive),
            source(&negative),
        ],
        source(&positive).shape,
        0,
        2,
        1024,
        || false,
    )
    .unwrap();
    assert_eq!(output[4], [0; 4]);
}

#[test]
fn exact_budget_and_validated_tail_spans_remain_bounded() {
    let pcm = [0; 4];
    let sources = [source(&pcm); 4];
    let needed = required_bytes(&sources, 2).unwrap();
    assert!(convert(sources, source(&pcm).shape, 0, 2, needed, || false).is_ok());
    assert_eq!(
        convert(sources, source(&pcm).shape, 0, 2, needed - 1, || false),
        Err(Error::Budget)
    );
    let mut wrong = sources;
    wrong[1].first = 1;
    assert_eq!(
        convert(wrong, source(&pcm).shape, 0, 2, needed, || false),
        Err(Error::Span)
    );
    assert_eq!(
        convert(sources, source(&pcm).shape, u64::MAX, 2, needed, || false),
        Err(Error::Shape)
    );
    for start in [(1u64 << 53) - 1, 1u64 << 53, (1u64 << 53) + 1] {
        let large = Shape {
            rate: 11,
            channels: 1,
            frames: start + 64,
        };
        let (first, count) = span(large, 7, start, 17);
        assert_eq!((first, count), (large.frames, 0));
    }
}

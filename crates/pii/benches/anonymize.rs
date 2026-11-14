//! Anonymizer benches: operator comparison + variant tuning.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use dbmcp_pii::{Analyzer, ChunkCount, Operator, RecognizerResult, anonymize};

mod common;

use common::{mixed_payload, sample_results};

const PAYLOAD_BYTES: usize = 16 * 1024;

fn run_group(c: &mut Criterion, name: &str, payload: &str, results: &[RecognizerResult], cases: &[(&str, Operator)]) {
    let mut g = c.benchmark_group(name);
    g.throughput(Throughput::Bytes(payload.len() as u64));
    for (label, op) in cases {
        g.bench_with_input(BenchmarkId::from_parameter(label), &payload, |b, text| {
            b.iter_batched(
                || results.to_vec(),
                |r| anonymize(black_box(text), r, black_box(op)),
                BatchSize::SmallInput,
            );
        });
    }
    g.finish();
}

fn bench_anonymize(c: &mut Criterion) {
    let analyzer = Analyzer::with_defaults();
    let payload = mixed_payload(PAYLOAD_BYTES);
    let results = sample_results(&analyzer, &payload);

    let operators: [(&str, Operator); 4] = [
        ("replace", Operator::Replace),
        ("mask", Operator::default_mask()),
        ("redact", Operator::Redact),
        ("hash", Operator::hash(b"0123456789abcdef0123456789abcdef")),
    ];
    let mask_variants: [(&str, Operator); 3] = [
        (
            "all_from_end",
            Operator::Mask {
                masking_char: '*',
                chars_to_mask: ChunkCount::All,
                from_end: true,
            },
        ),
        (
            "n4_from_end",
            Operator::Mask {
                masking_char: '*',
                chars_to_mask: ChunkCount::N(4),
                from_end: true,
            },
        ),
        (
            "n4_from_start",
            Operator::Mask {
                masking_char: '*',
                chars_to_mask: ChunkCount::N(4),
                from_end: false,
            },
        ),
    ];

    run_group(c, "anonymize/operators", &payload, &results, &operators);
    run_group(c, "anonymize/mask_chunk_count", &payload, &results, &mask_variants);
}

criterion_group!(benches, bench_anonymize);
criterion_main!(benches);

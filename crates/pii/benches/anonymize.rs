//! Anonymizer benches: operator comparison + variant tuning.

use std::borrow::Cow;
use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use dbmcp_pii::{Analyzer, ChunkCount, HashAlgorithm, Operator, OperatorConfig, RecognizerResult, anonymize};

mod common;

use common::{mixed_payload, sample_results};

const PAYLOAD_BYTES: usize = 16 * 1024;

fn run_group(c: &mut Criterion, name: &str, payload: &str, results: &[RecognizerResult], cases: &[(&str, Operator)]) {
    let mut g = c.benchmark_group(name);
    g.throughput(Throughput::Bytes(payload.len() as u64));
    for (label, op) in cases {
        let cfg = OperatorConfig {
            default: Some(op.clone()),
            ..OperatorConfig::default()
        };
        g.bench_with_input(BenchmarkId::from_parameter(label), &payload, |b, text| {
            b.iter_batched(
                || results.to_vec(),
                |r| anonymize(black_box(text), r, black_box(&cfg)),
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
        (
            "replace",
            Operator::Replace {
                new_value: Cow::Borrowed("<REDACTED>"),
            },
        ),
        ("mask", Operator::default_mask()),
        ("redact", Operator::Redact),
        ("hash_sha256", Operator::hash(HashAlgorithm::Sha256)),
    ];
    let hash_algorithms: [(&str, Operator); 2] = [
        ("sha256", Operator::hash(HashAlgorithm::Sha256)),
        ("sha512", Operator::hash(HashAlgorithm::Sha512)),
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
    run_group(c, "anonymize/hash_algorithms", &payload, &results, &hash_algorithms);
    run_group(c, "anonymize/mask_chunk_count", &payload, &results, &mask_variants);
}

criterion_group!(benches, bench_anonymize);
criterion_main!(benches);

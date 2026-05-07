//! Analyzer benches: recognizer match throughput, per-rule cost, build cost.

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use dbmcp_pii::{AnalyzeOptions, Analyzer, Category, overlap};

mod common;

use common::{SIZES, mixed_payload};

fn bench_all_recognizers(c: &mut Criterion) {
    let analyzer = Analyzer::with_defaults();
    let opts = AnalyzeOptions::default();

    let mut group = c.benchmark_group("analyze/all_recognizers");
    for &size in SIZES {
        let payload = mixed_payload(size);
        group.throughput(Throughput::Bytes(payload.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &payload, |b, text| {
            b.iter_with_large_drop(|| analyzer.analyze(black_box(text), black_box(&opts)));
        });
    }
    group.finish();
}

fn bench_by_rule(c: &mut Criterion) {
    let opts = AnalyzeOptions::default();
    let payload = mixed_payload(64 * 1024);
    let analyzer = Analyzer::with_defaults();

    let mut group = c.benchmark_group("analyze/by_rule");
    group.throughput(Throughput::Bytes(payload.len() as u64));
    group.sample_size(50);
    group.measurement_time(Duration::from_secs(3));

    for rule in analyzer.recognizers() {
        let label = rule.name().trim_end_matches("Recognizer");
        group.bench_with_input(BenchmarkId::from_parameter(label), &payload, |b, text| {
            b.iter_with_large_drop(|| {
                let mut results = rule.analyze(black_box(text));
                results.retain(|r| r.score >= opts.min_score);
                overlap::resolve(results)
            });
        });
    }
    group.finish();
}

fn bench_analyzer_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("analyze/build");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(2));

    group.bench_function("with_defaults", |b| {
        b.iter_with_large_drop(Analyzer::with_defaults);
    });

    group.bench_function("builder_filtered_financial", |b| {
        b.iter_with_large_drop(|| {
            Analyzer::builder()
                .categories([Category::Financial])
                .build()
                .expect("build")
        });
    });
    group.finish();
}

criterion_group!(benches, bench_all_recognizers, bench_by_rule, bench_analyzer_build,);
criterion_main!(benches);

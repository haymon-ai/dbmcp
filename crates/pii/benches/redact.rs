//! Redactor benches: end-to-end JSON walker on production-shaped payloads.

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use dbmcp_pii::Redactor;
use serde_json::{Map, Value, json};

mod common;

use common::pii_pool;

const POOL_STEMS: &[&str] = &["email", "ip", "credit_card", "iban"];

fn flat_rows(pool: &[String]) -> Vec<Value> {
    const ROWS: usize = 1000;
    const COLS: usize = 10;
    let mut out = Vec::with_capacity(ROWS);
    for r in 0..ROWS {
        let mut map = Map::new();
        for c in 0..COLS {
            let key = format!("col_{c}");
            let cell = if (r * COLS + c).is_multiple_of(3) {
                pool[(r + c) % pool.len()].clone()
            } else {
                format!("plain text cell {r}/{c} no pii here")
            };
            map.insert(key, Value::String(cell));
        }
        out.push(Value::Object(map));
    }
    out
}

fn nested_jsonb_rows(pool: &[String]) -> Vec<Value> {
    const ROWS: usize = 100;
    let mut out = Vec::with_capacity(ROWS);
    for r in 0..ROWS {
        let leaf_pii = pool[r % pool.len()].clone();
        let leaf_plain = format!("user_event_{r}");
        let payload = json!({
            "id": r,
            "ts": "2026-05-07T10:00:00Z",
            "data": {
                "user": {
                    "profile": {
                        "contact": leaf_pii,
                        "label": leaf_plain,
                    },
                    "tags": ["alpha", "beta", pool[(r + 1) % pool.len()].clone()],
                },
                "audit": [
                    {"event": "login", "src": pool[(r + 2) % pool.len()].clone()},
                    {"event": "view", "src": "192.0.2.0"},
                ],
            },
        });
        out.push(payload);
    }
    out
}

fn large_blob_rows(pool: &[String]) -> Vec<Value> {
    const ROWS: usize = 10;
    const TARGET: usize = 64 * 1024;
    let mut out = Vec::with_capacity(ROWS);
    for r in 0..ROWS {
        let mut blob = String::with_capacity(TARGET + 256);
        let mut cycle = pool.iter().cycle().skip(r);
        while blob.len() < TARGET {
            blob.push_str("lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor ");
            blob.push_str(cycle.next().expect("non-empty pool"));
            blob.push(' ');
        }
        out.push(json!({"row": r, "blob": blob}));
    }
    out
}

fn bench_redact_shapes(c: &mut Criterion) {
    let redactor = Redactor::with_defaults();
    let pool = pii_pool(POOL_STEMS);

    let shapes: [(&str, Vec<Value>); 3] = [
        ("flat_rows", flat_rows(&pool)),
        ("nested_jsonb", nested_jsonb_rows(&pool)),
        ("large_blob", large_blob_rows(&pool)),
    ];

    let mut group = c.benchmark_group("redact/shapes");
    for (label, rows) in &shapes {
        group.throughput(Throughput::Elements(rows.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), rows, |b, rs| {
            b.iter_batched_ref(
                || rs.clone(),
                |r| {
                    redactor
                        .apply(black_box(r))
                        .expect("redactor must not panic on bench input")
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_redact_shapes);
criterion_main!(benches);

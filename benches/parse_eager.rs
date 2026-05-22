//! End-to-end parse benchmark across ASCII and CJK fixtures, in both
//! EAGER and LAZY mode. Used to measure the cost of value-level
//! validation (the EAGER-vs-LAZY gap) — which is what the upcoming
//! SIMD UTF-8 validator targets — and to guard against ASCII-path
//! regressions.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use qjson::doc::Document;
use qjson::options::{
    Options, QJSON_DEFAULT_MAX_DEPTH, QJSON_MODE_EAGER, QJSON_MODE_LAZY,
};
use std::fs;

const FIXTURES: &[(&str, &str)] = &[
    ("ascii", "benches/fixtures/medium_resp.json"),
    ("cjk",   "benches/fixtures/medium_resp_cjk.json"),
    ("mixed", "benches/fixtures/medium_resp_mixed.json"),
    ("emoji", "benches/fixtures/medium_resp_emoji.json"),
];

fn run(c: &mut Criterion) {
    for (name, path) in FIXTURES {
        let buf = fs::read(path)
            .unwrap_or_else(|e| panic!("read {}: {}", path, e));
        let mut group = c.benchmark_group(format!("parse/{}", name));
        group.throughput(Throughput::Bytes(buf.len() as u64));

        let eager = Options {
            mode:      QJSON_MODE_EAGER,
            max_depth: QJSON_DEFAULT_MAX_DEPTH,
        };
        let lazy = Options {
            mode:      QJSON_MODE_LAZY,
            max_depth: QJSON_DEFAULT_MAX_DEPTH,
        };

        group.bench_function("eager", |b| {
            b.iter(|| {
                let doc = Document::parse_with_options(black_box(&buf), &eager)
                    .expect("parse eager");
                black_box(doc);
            })
        });
        group.bench_function("lazy", |b| {
            b.iter(|| {
                let doc = Document::parse_with_options(black_box(&buf), &lazy)
                    .expect("parse lazy");
                black_box(doc);
            })
        });

        group.finish();
    }
}

criterion_group!(benches, run);
criterion_main!(benches);

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use qjson::__bench_api::{Document, Options, QJSON_MODE_LAZY};
use std::fs;

fn read_fixture(path: &str) -> Vec<u8> {
    fs::read(path).unwrap_or_else(|e| panic!("failed to read {}: {}", path, e))
}

struct Fixture {
    name: &'static str,
    #[allow(dead_code)]
    path: &'static str,
    data: Vec<u8>,
}

fn load_fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "small_api",
            path: "benches/fixtures/small_api.json",
            data: read_fixture("benches/fixtures/small_api.json"),
        },
        Fixture {
            name: "wide_object",
            path: "tests/fixtures/data/wide_object.json",
            data: read_fixture("tests/fixtures/data/wide_object.json"),
        },
        Fixture {
            name: "deep_nesting",
            path: "tests/fixtures/data/deep_nesting.json",
            data: read_fixture("tests/fixtures/data/deep_nesting.json"),
        },
    ]
}

fn bench_parse_eager(c: &mut Criterion) {
    let fixtures = load_fixtures();
    let mut group = c.benchmark_group("parse_eager");

    for f in &fixtures {
        group.bench_with_input(BenchmarkId::new("parse", f.name), &f.data, |b, data| {
            b.iter(|| Document::parse(black_box(data)).unwrap())
        });
    }

    group.finish();
}

fn bench_parse_lazy(c: &mut Criterion) {
    let fixtures = load_fixtures();
    let opts = Options { mode: QJSON_MODE_LAZY, max_depth: 0 };
    let mut group = c.benchmark_group("parse_lazy");

    for f in &fixtures {
        group.bench_with_input(BenchmarkId::new("parse", f.name), &f.data, |b, data| {
            b.iter(|| Document::parse_with_options(black_box(data), &opts).unwrap())
        });
    }

    group.finish();
}

fn bench_field_access(c: &mut Criterion) {
    let small = read_fixture("benches/fixtures/small_api.json");
    let doc = Document::parse(&small).unwrap();
    let mut group = c.benchmark_group("field_access");

    group.bench_function("get_str/model", |b| {
        b.iter(|| {
            let mut out_ptr = std::ptr::null();
            let mut out_len = 0usize;
            unsafe {
                qjson::ffi::qjson_get_str(
                    &doc as *const _ as *mut _,
                    b"model".as_ptr() as *const _,
                    5,
                    &mut out_ptr,
                    &mut out_len,
                )
            }
        })
    });

    group.bench_function("get_f64/max_tokens", |b| {
        b.iter(|| {
            let mut out = 0f64;
            unsafe {
                qjson::ffi::qjson_get_f64(
                    &doc as *const _ as *mut _,
                    b"max_tokens".as_ptr() as *const _,
                    10,
                    &mut out,
                )
            }
        })
    });

    group.bench_function("get_str/nested", |b| {
        b.iter(|| {
            let mut out_ptr = std::ptr::null();
            let mut out_len = 0usize;
            unsafe {
                qjson::ffi::qjson_get_str(
                    &doc as *const _ as *mut _,
                    b"messages[0].role".as_ptr() as *const _,
                    16,
                    &mut out_ptr,
                    &mut out_len,
                )
            }
        })
    });

    group.finish();
}

criterion_group!(benches, bench_parse_eager, bench_parse_lazy, bench_field_access);
criterion_main!(benches);

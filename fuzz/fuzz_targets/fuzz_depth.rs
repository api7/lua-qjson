#![no_main]

use std::os::raw::{c_char, c_int};
use std::ptr;
use std::sync::Once;

use libfuzzer_sys::fuzz_target;
use qjson::doc::Document;
use qjson::error::qjson_err;
use qjson::ffi::{
    qjson_cursor, qjson_cursor_bytes, qjson_cursor_field, qjson_cursor_index, qjson_cursor_len,
    qjson_free, qjson_open, qjson_parse_ex,
};
use qjson::options::{
    Options, QJSON_DEFAULT_MAX_DEPTH, QJSON_MAX_MAX_DEPTH, QJSON_MODE_EAGER, QJSON_MODE_LAZY,
};

static BOUNDARY_ASSERTIONS: Once = Once::new();

fuzz_target!(|data: &[u8]| {
    BOUNDARY_ASSERTIONS.call_once(run_boundary_assertions);
    run_generated_case(data);
});

fn run_boundary_assertions() {
    for mode in [QJSON_MODE_EAGER, QJSON_MODE_LAZY] {
        assert_boundary(mode, 0, QJSON_DEFAULT_MAX_DEPTH, ShapeMode::Array);
        assert_boundary(
            mode,
            QJSON_MAX_MAX_DEPTH,
            QJSON_MAX_MAX_DEPTH,
            ShapeMode::Object,
        );
        assert_boundary(mode, u32::MAX, QJSON_MAX_MAX_DEPTH, ShapeMode::Mixed);
    }
}

fn assert_boundary(
    mode: u32,
    requested_max_depth: u32,
    effective_max_depth: u32,
    shape: ShapeMode,
) {
    let opts = Options {
        mode,
        max_depth: requested_max_depth,
    };

    let at_limit = nested_json(effective_max_depth, shape, &[0]);
    assert_parse_ok(&at_limit, &opts);
    walk_accepted_doc(&at_limit, &opts, shape.path_steps(effective_max_depth));

    let over_limit = nested_json(effective_max_depth + 1, shape, &[0]);
    assert_nesting_error(&over_limit, &opts);
}

fn run_generated_case(data: &[u8]) {
    let opts = Options {
        mode: if byte(data, 0) & 1 == 0 {
            QJSON_MODE_EAGER
        } else {
            QJSON_MODE_LAZY
        },
        max_depth: requested_max_depth(data),
    };
    let effective = effective_max_depth(opts.max_depth);
    let depth = generated_depth(data, effective);
    let shape = ShapeMode::from_byte(byte(data, 4));
    let mut json = nested_json(depth, shape, data);
    let mutated = mutate_skeleton(&mut json, data);

    let result = Document::parse_with_options(&json, &opts);
    let ffi_err = ffi_parse_error(&json, &opts);
    assert_ne!(
        ffi_err,
        qjson_err::QJSON_OOM as c_int,
        "panic barrier tripped for {json:?}"
    );

    if !mutated {
        if depth > effective {
            assert_eq!(result.err(), Some(qjson_err::QJSON_NESTING_TOO_DEEP));
            assert_eq!(ffi_err, qjson_err::QJSON_NESTING_TOO_DEEP as c_int);
        } else {
            assert!(
                result.is_ok(),
                "valid depth {depth} <= {effective} was rejected"
            );
            assert_eq!(ffi_err, qjson_err::QJSON_OK as c_int);
            if depth <= 256 {
                walk_accepted_doc(
                    &json,
                    &opts,
                    (0..depth).map(|level| shape.step(level, data)),
                );
            }
        }
    }
}

fn assert_parse_ok(json: &[u8], opts: &Options) {
    assert!(
        Document::parse_with_options(json, opts).is_ok(),
        "depth at limit should parse: mode={} max_depth={}",
        opts.mode,
        opts.max_depth,
    );
    assert_eq!(ffi_parse_error(json, opts), qjson_err::QJSON_OK as c_int);
}

fn assert_nesting_error(json: &[u8], opts: &Options) {
    assert_eq!(
        Document::parse_with_options(json, opts).err(),
        Some(qjson_err::QJSON_NESTING_TOO_DEEP),
        "one past depth limit should return QJSON_NESTING_TOO_DEEP: mode={} max_depth={}",
        opts.mode,
        opts.max_depth,
    );
    assert_eq!(
        ffi_parse_error(json, opts),
        qjson_err::QJSON_NESTING_TOO_DEEP as c_int,
        "FFI one-past-limit parse should surface the same clean error",
    );
}

fn ffi_parse_error(json: &[u8], opts: &Options) -> c_int {
    unsafe {
        let mut err = -1;
        let doc = qjson_parse_ex(json.as_ptr(), json.len(), opts as *const Options, &mut err);
        if !doc.is_null() {
            qjson_free(doc);
        }
        err
    }
}

fn walk_accepted_doc(json: &[u8], opts: &Options, steps: impl Iterator<Item = Step>) {
    unsafe {
        let mut err = -1;
        let doc = qjson_parse_ex(json.as_ptr(), json.len(), opts as *const Options, &mut err);
        assert!(
            !doc.is_null(),
            "accepted Phase 2 sample failed to parse: err={err}"
        );
        assert_eq!(err, qjson_err::QJSON_OK as c_int);

        let mut cur: qjson_cursor = std::mem::zeroed();
        assert_eq!(
            qjson_open(doc, ptr::null(), 0, &mut cur),
            qjson_err::QJSON_OK as c_int
        );

        let key = b"k";
        for step in steps {
            let mut len = usize::MAX;
            assert_eq!(
                qjson_cursor_len(&cur, ptr::null(), 0, &mut len),
                qjson_err::QJSON_OK as c_int,
            );
            assert_eq!(len, 1);

            let mut next: qjson_cursor = std::mem::zeroed();
            let rc = match step {
                Step::Index0 => qjson_cursor_index(&cur, 0, &mut next),
                Step::KeyK => {
                    qjson_cursor_field(&cur, key.as_ptr() as *const c_char, key.len(), &mut next)
                }
            };
            assert_eq!(rc, qjson_err::QJSON_OK as c_int);
            cur = next;
        }

        let mut byte_start = usize::MAX;
        let mut byte_end = usize::MAX;
        assert_eq!(
            qjson_cursor_bytes(&cur, &mut byte_start, &mut byte_end),
            qjson_err::QJSON_OK as c_int,
        );
        assert!(byte_start < byte_end && byte_end <= json.len());
        qjson_free(doc);
    }
}

fn requested_max_depth(data: &[u8]) -> u32 {
    match byte(data, 1) % 5 {
        0 => 0,
        1 => QJSON_DEFAULT_MAX_DEPTH,
        2 => QJSON_MAX_MAX_DEPTH,
        3 => u32::MAX,
        _ => 1 + u32::from(u16_at(data, 2)) % QJSON_MAX_MAX_DEPTH,
    }
}

fn effective_max_depth(requested: u32) -> u32 {
    let depth = if requested == 0 {
        QJSON_DEFAULT_MAX_DEPTH
    } else {
        requested
    };
    depth.min(QJSON_MAX_MAX_DEPTH)
}

fn generated_depth(data: &[u8], effective: u32) -> u32 {
    match byte(data, 5) % 8 {
        0 => effective.saturating_sub(1).max(1),
        1 => effective,
        2 => effective + 1,
        3 => effective + 2,
        _ => 1 + u32::from(u16_at(data, 6)) % (QJSON_MAX_MAX_DEPTH + 2),
    }
}

fn nested_json(depth: u32, shape: ShapeMode, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity((depth as usize).saturating_mul(6).saturating_add(8));
    let mut closers = Vec::with_capacity(depth as usize);

    for level in 0..depth {
        match shape.step(level, data) {
            Step::Index0 => {
                out.push(b'[');
                closers.push(b']');
            }
            Step::KeyK => {
                out.extend_from_slice(br#"{"k":"#);
                closers.push(b'}');
            }
        }
    }

    match byte(data, 8) % 4 {
        0 => out.push(b'0'),
        1 => out.extend_from_slice(b"true"),
        2 => out.extend_from_slice(b"null"),
        _ => out.extend_from_slice(br#""x""#),
    }

    for closer in closers.into_iter().rev() {
        out.push(closer);
    }

    out
}

fn mutate_skeleton(json: &mut Vec<u8>, data: &[u8]) -> bool {
    if json.is_empty() {
        return false;
    }

    match byte(data, 9) % 5 {
        0 => false,
        1 => {
            let new_len = usize::from(u16_at(data, 10)) % json.len();
            json.truncate(new_len);
            true
        }
        2 => {
            let pos = usize::from(u16_at(data, 10)) % json.len();
            json[pos] = match json[pos] {
                b'[' => b'{',
                b'{' => b'[',
                b']' => b'}',
                b'}' => b']',
                b'"' => b'\\',
                _ => b'[',
            };
            true
        }
        3 => {
            json.push(b' ');
            json.push(b'x');
            true
        }
        _ => {
            let pos = usize::from(u16_at(data, 10)) % (json.len() + 1);
            json.insert(pos, byte(data, 12));
            true
        }
    }
}

fn byte(data: &[u8], idx: usize) -> u8 {
    data.get(idx).copied().unwrap_or(0)
}

fn u16_at(data: &[u8], idx: usize) -> u16 {
    u16::from_le_bytes([byte(data, idx), byte(data, idx + 1)])
}

#[derive(Copy, Clone)]
enum ShapeMode {
    Array,
    Object,
    Mixed,
}

impl ShapeMode {
    fn from_byte(byte: u8) -> Self {
        match byte % 3 {
            0 => Self::Array,
            1 => Self::Object,
            _ => Self::Mixed,
        }
    }

    fn step(self, level: u32, data: &[u8]) -> Step {
        match self {
            Self::Array => Step::Index0,
            Self::Object => Step::KeyK,
            Self::Mixed => {
                let selector = byte(data, 13 + (level as usize % 16));
                if (selector >> (level % 8)) & 1 == 0 {
                    Step::Index0
                } else {
                    Step::KeyK
                }
            }
        }
    }

    fn path_steps(self, depth: u32) -> impl Iterator<Item = Step> {
        (0..depth).map(move |level| self.step(level, &[]))
    }
}

#[derive(Copy, Clone)]
enum Step {
    Index0,
    KeyK,
}

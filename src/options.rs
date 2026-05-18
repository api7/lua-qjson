#![allow(non_camel_case_types)]

pub const QJD_MODE_EAGER: u32 = 0;
pub const QJD_MODE_LAZY:  u32 = 1;
pub const QJD_DEFAULT_MAX_DEPTH: u32 = 1024;
pub const QJD_MAX_MAX_DEPTH:     u32 = 4096;

/// Caller-visible parse options. Layout is FFI-stable: kept in sync with
/// `qjd_options` in `include/lua_quick_decode.h`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// `QJD_MODE_EAGER` (0) — full RFC 8259 validation during parse.
    /// `QJD_MODE_LAZY`  (1) — structural-only; defer value errors to access.
    pub mode: u32,
    /// Max bracket nesting depth. `0` selects `QJD_DEFAULT_MAX_DEPTH` (1024).
    /// Values >`QJD_MAX_MAX_DEPTH` are clamped to that ceiling.
    pub max_depth: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self { mode: QJD_MODE_EAGER, max_depth: 0 }
    }
}

#[allow(dead_code)] // used in Task 6+ validators
impl Options {
    pub(crate) fn effective_max_depth(&self) -> u32 {
        let d = if self.max_depth == 0 { QJD_DEFAULT_MAX_DEPTH } else { self.max_depth };
        d.min(QJD_MAX_MAX_DEPTH)
    }

    pub(crate) fn is_eager(&self) -> bool {
        self.mode == QJD_MODE_EAGER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn default_is_eager() { assert!(Options::default().is_eager()); }

    #[test]
    fn zero_max_depth_falls_back_to_default() {
        assert_eq!(Options::default().effective_max_depth(), QJD_DEFAULT_MAX_DEPTH);
    }

    #[test]
    fn huge_max_depth_is_clamped() {
        let o = Options { mode: 0, max_depth: u32::MAX };
        assert_eq!(o.effective_max_depth(), QJD_MAX_MAX_DEPTH);
    }
}

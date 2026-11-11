//! Choice of HF waveform keying, shared by `xara` and `xara-monitor`.
//!
//! VARA HF 4.9.1 switches to a new keying when its clock reaches 2027. The
//! tables for it cannot be generated and are not shipped with xara: the user
//! points `--vara-dir` at their own VARA HF 4.9.1 installation.
//!
//! `--vara-dir` may instead point at a named keying made by `xara-keygen`.
//! That is the user's explicit choice of waveform, so it applies whatever the
//! date, unless `--waveform legacy` is given.

use std::path::Path;
use std::str::FromStr;
use xara_phy::hf::Keying;

pub const KEYING_YEAR: i32 = 2027;

/// `--waveform`: which keying to use. `Auto` follows VARA's own rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Choice {
    #[default]
    Auto,
    Legacy,
    Y2027,
}

impl FromStr for Choice {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Choice::Auto),
            "legacy" => Ok(Choice::Legacy),
            "2027" => Ok(Choice::Y2027),
            w => Err(format!("bad waveform {w} (auto, legacy or 2027)")),
        }
    }
}

impl Choice {
    pub fn wants_2027(self, year: i32) -> bool {
        match self {
            Choice::Auto => year >= KEYING_YEAR,
            Choice::Legacy => false,
            Choice::Y2027 => true,
        }
    }

    /// Reject `--waveform 2027` without `--vara-dir` at option-parsing time.
    pub fn check(self, vara_dir: Option<&Path>) -> Result<(), String> {
        match (self, vara_dir) {
            (Choice::Y2027, None) => Err("--waveform 2027 needs --vara-dir".into()),
            _ => Ok(()),
        }
    }
}

fn local_year() -> i32 {
    // SAFETY: time() accepts a null pointer, and localtime_r() only writes to the tm we pass.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() { 1970 } else { tm.tm_year + 1900 }
    }
}

/// Pick the HF keying for this process. Call once, before any HF modem,
/// monitor or codebook is built. Returns whether the 2027 keying is active.
pub fn select(vara_dir: Option<&Path>, choice: Choice) -> Result<bool, String> {
    let year = local_year();
    let named = vara_dir.is_some_and(Keying::is_named);
    match (vara_dir, choice.wants_2027(year) || (named && choice != Choice::Legacy)) {
        (Some(dir), true) => {
            let k = Keying::load(dir).map_err(|e| format!("--vara-dir: {e}"))?;
            let what = if named { "named keying" } else { "2027 keying" };
            say!("HF waveform: {what}, tables from {} (control seed offset {})", dir.display(), k.seed_bias());
            xara_phy::hf::install_keying(k)?;
            Ok(true)
        }
        (None, true) => {
            say!(
                "warning: it is {year}; VARA HF 4.9.1 and later use a new waveform from {KEYING_YEAR}. \
                 set --vara-dir to interoperate with them (see README)"
            );
            Ok(false)
        }
        (_, false) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice() {
        assert_eq!("auto".parse(), Ok(Choice::Auto));
        assert_eq!("Legacy".parse(), Ok(Choice::Legacy));
        assert_eq!("2027".parse(), Ok(Choice::Y2027));
        assert!("2026".parse::<Choice>().is_err());
        assert!(!Choice::Auto.wants_2027(2026));
        assert!(Choice::Auto.wants_2027(2027));
        assert!(!Choice::Legacy.wants_2027(2030));
        assert!(Choice::Y2027.wants_2027(2026));
        assert!(Choice::Y2027.check(None).is_err());
        assert!(Choice::Y2027.check(Some(Path::new("/x"))).is_ok());
        assert!(Choice::Auto.check(None).is_ok());
    }
}

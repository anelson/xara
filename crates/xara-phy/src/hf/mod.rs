pub mod ctrl;
pub mod frame;
pub mod fsk;
pub mod level;
pub mod ofdm;
pub mod psk;
pub mod rx;
pub mod tx;

use crate::dat::{PermOrder, RecordTables, hf_tables, hf_tables_from_dat, hf_tables_with, serialize_ordered};
use crate::rnd::{Draw, SplitMix64, fnv1a64};
use crate::seq;
use level::Bandwidth;
use num_complex::Complex64;
use std::f64::consts::PI;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, OnceLock};

type Phasors = Vec<Vec<Complex64>>;

/// Offset VARA HF 4.9.1 adds to every control-code seed from 2027.
pub const SEED_BIAS_2027: u64 = 687;

/// A waveform keying other than the one used up to VARA HF 4.9.0: interleaver
/// tables, phase tables and an offset added to every control-code seed.
///
/// There are two kinds, both read from a directory of table files:
///
/// - the keying VARA HF 4.9.1 switches to when its clock reaches 2027. Those
///   tables cannot be generated, so the user supplies them from their own
///   VARA HF 4.9.1 installation. xara does not ship them.
/// - a named keying made by [`Keying::generate`]. Stations only hear each
///   other if they load the same one. The name is the whole definition, so
///   publishing the name publishes the waveform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keying {
    tables: [Vec<RecordTables>; 3],
    psk: Vec<[u8; seq::PSK_SYMBOLS]>,
    ofdm: Vec<[u8; seq::OFDM_SYMBOLS]>,
    seed_bias: u64,
}

/// One slice of a phase table in file order: both tables are stored as a
/// first block of symbols for every row, then the remaining symbols.
fn phase_blocks<const N: usize>(split: usize) -> [std::ops::Range<usize>; 2] {
    [0..split, split..N]
}

fn flatten<const N: usize>(rows: &[[u8; N]], split: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(rows.len() * N);
    for ks in phase_blocks::<N>(split) {
        for row in rows {
            out.extend_from_slice(&row[ks.clone()]);
        }
    }
    out
}

impl Keying {
    /// Table file for each bandwidth, in [`Bandwidth::ALL`] order.
    pub const TABLE_FILES: [&'static str; 3] = ["Binary1.dat", "Binary2.dat", "Binary3.dat"];
    pub const OFDM_FILE: &'static str = "Mod1.dat";
    pub const PSK_FILE: &'static str = "Mod2.dat";
    /// Written next to generated tables; marks a directory as a named keying
    /// and carries its control-code seed offset.
    pub const INFO_FILE: &'static str = "xara-keying.txt";
    /// Version of the generation algorithm. A name always produces the same
    /// tables for a given version; any change to the algorithm must bump it.
    pub const GENERATOR: u32 = 1;
    const PSK_SPLIT: usize = 411;
    const OFDM_SPLIT: usize = 409;

    pub fn from_bytes(tables: [&[u8]; 3], ofdm: &[u8], psk: &[u8], seed_bias: u64) -> Result<Self, String> {
        let mut out: [Vec<RecordTables>; 3] = Default::default();
        for ((o, bw), (dat, name)) in out.iter_mut().zip(Bandwidth::ALL).zip(tables.into_iter().zip(Self::TABLE_FILES))
        {
            *o = hf_tables_from_dat(bw, dat, PermOrder::TurboFirst).map_err(|e| format!("{name}: {e}"))?;
        }
        Ok(Self {
            tables: out,
            psk: seq::psk_phasor_index_from(psk)?,
            ofdm: seq::ofdm_phasor_index_from(ofdm)?,
            seed_bias,
        })
    }

    /// Whether `dir` holds a named keying made by xara rather than VARA's files.
    pub fn is_named(dir: &Path) -> bool {
        dir.join(Self::INFO_FILE).is_file()
    }

    /// Read a keying from a directory: either a VARA HF 4.9.1 installation, or
    /// a directory written by [`Keying::write_new`].
    pub fn load(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            let p = dir.join(name);
            std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
        };
        let seed_bias = match Self::is_named(dir) {
            true => {
                let text =
                    String::from_utf8(read(Self::INFO_FILE)?).map_err(|_| format!("{}: not text", Self::INFO_FILE))?;
                info_value(&text, "seed-offset")?
            }
            false => SEED_BIAS_2027,
        };
        let [b1, b2, b3] = Self::TABLE_FILES.map(read);
        let (b1, b2, b3) = (b1?, b2?, b3?);
        Self::from_bytes([&b1, &b2, &b3], &read(Self::OFDM_FILE)?, &read(Self::PSK_FILE)?, seed_bias)
    }

    /// Generate the keying called `name`. Deterministic: the same name gives
    /// the same keying on every machine (for one [`Keying::GENERATOR`]).
    ///
    /// Each part draws from its own SplitMix64 stream, seeded with the FNV-1a
    /// hash of `"xara-keying/<GENERATOR>/<part>/<name>"`.
    pub fn generate(name: &str) -> Self {
        let stream =
            |part: &str| SplitMix64::new(fnv1a64(format!("xara-keying/{}/{part}/{name}", Self::GENERATOR).as_bytes()));
        let tables = Bandwidth::ALL.map(|bw| hf_tables_with(bw, &mut stream(&format!("tables-{}", bw.hz()))));
        let mut r = stream("psk");
        let mut psk = vec![[0u8; seq::PSK_SYMBOLS]; seq::PSK_CARRIERS];
        psk.iter_mut().flatten().for_each(|v| *v = r.draw(129) as u8);
        let mut r = stream("ofdm");
        let mut ofdm = vec![[0u8; seq::OFDM_SYMBOLS]; seq::OFDM_BINS];
        ofdm.iter_mut().flatten().for_each(|v| *v = r.draw(129) as u8);
        // Keep clear of 0 (the keying up to 4.9.0) and of VARA's own offset.
        let seed_bias = 1024 + u64::from(stream("seed-offset").draw(31_000));
        Self { tables, psk, ofdm, seed_bias }
    }

    /// The offset this keying adds to every control-code seed.
    pub fn seed_bias(&self) -> u64 {
        self.seed_bias
    }

    /// The five table files, in the format [`Keying::load`] reads.
    pub fn files(&self) -> [(&'static str, Vec<u8>); 5] {
        let [t1, t2, t3] = Self::TABLE_FILES;
        let [b1, b2, b3] = [0, 1, 2].map(|i| serialize_ordered(&self.tables[i], PermOrder::TurboFirst));
        [
            (t1, b1),
            (t2, b2),
            (t3, b3),
            (Self::OFDM_FILE, flatten(&self.ofdm, Self::OFDM_SPLIT)),
            (Self::PSK_FILE, flatten(&self.psk, Self::PSK_SPLIT)),
        ]
    }

    /// Generate the keying called `name` and write it to `dir`, which is
    /// created if needed. Refuses to overwrite: `dir` must not already hold
    /// any of the files, so a VARA installation can never be damaged.
    pub fn write_new(dir: &Path, name: &str) -> Result<Self, String> {
        check_name(name)?;
        let k = Self::generate(name);
        let at = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
        std::fs::create_dir_all(dir).map_err(|e| at(dir, e))?;
        let info = format!(
            "# xara named keying. Anyone can rebuild these tables with:\n\
             #   xara-keygen --name \"{name}\" --out DIR\n\
             name = {name}\n\
             generator = {}\n\
             seed-offset = {}\n",
            Self::GENERATOR,
            k.seed_bias
        );
        let files = k.files().into_iter().chain([(Self::INFO_FILE, info.into_bytes())]);
        let files: Vec<_> = files.map(|(n, b)| (dir.join(n), b)).collect();
        if let Some((p, _)) = files.iter().find(|(p, _)| p.exists()) {
            return Err(format!("{} already exists; choose an empty directory", p.display()));
        }
        for (p, b) in &files {
            std::fs::write(p, b).map_err(|e| at(p, e))?;
        }
        Ok(k)
    }
}

/// Names are published and typed by hand, so keep them unambiguous.
pub fn check_name(name: &str) -> Result<(), String> {
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    match name.len() {
        1..=64 if name.chars().all(ok) => Ok(()),
        _ => Err("keying name must be 1-64 characters of A-Z, a-z, 0-9, '-', '_' and '.'".into()),
    }
}

fn info_value(text: &str, key: &str) -> Result<u64, String> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .find(|(k, _)| k.trim() == key)
        .and_then(|(_, v)| v.trim().parse().ok())
        .ok_or_else(|| format!("{}: no valid {key}", Keying::INFO_FILE))
}

static KEYING: OnceLock<Keying> = OnceLock::new();
static KEYING_READ: AtomicBool = AtomicBool::new(false);
static TABLES: [OnceLock<Vec<RecordTables>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
static WHITE: LazyLock<Vec<u8>> = LazyLock::new(|| seq::whitening(seq::HF_SEED));
static PSK_PHASORS: LazyLock<Phasors> =
    LazyLock::new(|| phasors(keying().map_or_else(seq::psk_phasor_index, |k| k.psk.clone())));
static OFDM_PHASORS: LazyLock<Phasors> =
    LazyLock::new(|| phasors(keying().map_or_else(seq::ofdm_phasor_index, |k| k.ofdm.clone())));

fn keying() -> Option<&'static Keying> {
    KEYING_READ.store(true, Ordering::SeqCst);
    KEYING.get()
}

/// Switch this process to another keying. Must be called once, before any HF
/// modem, codebook or table is built: the choice is fixed for the life of the
/// process, as it is in VARA.
pub fn install_keying(k: Keying) -> Result<(), String> {
    if KEYING_READ.load(Ordering::SeqCst) {
        return Err("the HF waveform tables are already in use".into());
    }
    KEYING.set(k).map_err(|_| "a keying is already installed".to_string())
}

/// Whether a keying other than the one used up to VARA HF 4.9.0 is active.
pub fn keying_2027() -> bool {
    keying().is_some()
}

pub(crate) fn seed_bias() -> u64 {
    keying().map_or(0, |k| k.seed_bias)
}

fn phasors<const N: usize>(idx: Vec<[u8; N]>) -> Phasors {
    idx.iter()
        .map(|r| r.iter().map(|&d| Complex64::from_polar(1.0, 2.0 * PI * f64::from(d) / 128.0)).collect())
        .collect()
}

pub fn tables(bw: Bandwidth) -> &'static [RecordTables] {
    TABLES[bw as usize].get_or_init(|| keying().map_or_else(|| hf_tables(bw), |k| k.tables[bw as usize].clone()))
}

pub fn whitening() -> &'static [u8] {
    &WHITE
}

pub fn warm(bw: Bandwidth) {
    tables(bw);
    whitening();
    psk_phasors();
    ofdm_phasors();
}

pub fn psk_phasors() -> &'static [Vec<Complex64>] {
    &PSK_PHASORS
}

pub fn ofdm_phasors() -> &'static [Vec<Complex64>] {
    &OFDM_PHASORS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keyings() {
        let a = Keying::generate("NET-7");
        assert_eq!(a, Keying::generate("NET-7"), "generation is deterministic");
        let b = Keying::generate("NET-8");
        assert_ne!(a.seed_bias(), 0);
        assert_ne!(a.seed_bias(), SEED_BIAS_2027);
        for bw in Bandwidth::ALL {
            let i = bw as usize;
            assert_eq!(a.tables[i].len(), bw.table().len());
            for ((x, y), g) in a.tables[i].iter().zip(&b.tables[i]).zip(hf_tables(bw)) {
                assert_ne!(x.chan, y.chan, "names give different tables");
                assert_ne!(x.chan, g.chan, "not the 4.9.0 keying");
                assert_eq!((x.chan.len(), x.turbo.len(), x.other.len()), (g.chan.len(), g.turbo.len(), g.other.len()));
            }
        }
        assert_ne!(a.psk, b.psk);
        assert_ne!(a.ofdm, b.ofdm);
        assert_ne!(a.tables[1], a.tables[2], "bandwidths draw from separate streams");

        // Files round-trip through the same loader VARA's files go through.
        let [(_, b1), (_, b2), (_, b3), (_, m1), (_, m2)] = a.files();
        assert_eq!(Keying::from_bytes([&b1, &b2, &b3], &m1, &m2, a.seed_bias()), Ok(a.clone()));

        let dir = std::env::temp_dir().join(format!("xara-keying-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!Keying::is_named(&dir));
        assert_eq!(Keying::write_new(&dir, "NET-7"), Ok(a.clone()));
        assert!(Keying::is_named(&dir));
        assert_eq!(Keying::load(&dir), Ok(a));
        assert!(Keying::write_new(&dir, "NET-8").unwrap_err().contains("already exists"));
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(check_name("").is_err());
        assert!(check_name("two words").is_err());
        assert!(check_name("Net_7.b-2").is_ok());
    }

    /// Pins generator version 1: if this changes, published names stop
    /// meaning the same tables, so GENERATOR must be bumped instead.
    #[test]
    fn generator_v1_is_stable() {
        let k = Keying::generate("NET-7");
        let sum = k.files().iter().map(|(_, b)| fnv1a64(b)).fold(k.seed_bias(), |a, h| a.rotate_left(7) ^ h);
        assert_eq!((Keying::GENERATOR, sum), (1, PINNED_NET_7), "sum {sum:#x}");
    }

    const PINNED_NET_7: u64 = 0xb3ab_2141_17f1_9826;

    #[test]
    fn keying_from_bytes() {
        let dats = Bandwidth::ALL.map(|bw| serialize_ordered(&hf_tables(bw), PermOrder::TurboFirst));
        let data = seq::data_dat();
        let ofdm = &data[..seq::OFDM_BINS * seq::OFDM_SYMBOLS];
        let psk = vec![0u8; seq::PSK_CARRIERS * seq::PSK_SYMBOLS];
        let k = Keying::from_bytes([&dats[0], &dats[1], &dats[2]], ofdm, &psk, 5).unwrap();
        assert_eq!(k.seed_bias(), 5);
        for bw in Bandwidth::ALL {
            assert_eq!(k.tables[bw as usize], hf_tables(bw));
        }
        assert_eq!(k.ofdm, seq::ofdm_phasor_index());
        let swapped = Keying::from_bytes([&dats[1], &dats[0], &dats[2]], ofdm, &psk, 5).unwrap_err();
        assert!(swapped.starts_with("Binary1.dat"), "{swapped}");
        assert!(
            Keying::from_bytes([&dats[0], &dats[1], &dats[2]], &data, &psk, 5).is_err(),
            "Data.dat is not Mod1.dat"
        );
    }
}

#[cfg(test)]
mod user_files {
    use super::*;

    /// Runs only when XARA_VARA_DIR points at a VARA HF 4.9.1 installation;
    /// the files are never part of this repository.
    #[test]
    fn loads_user_supplied_keying() {
        let Some(dir) = std::env::var_os("XARA_VARA_DIR") else { return };
        let k = Keying::load(Path::new(&dir)).unwrap();
        for bw in Bandwidth::ALL {
            assert_eq!(k.tables[bw as usize].len(), bw.table().len());
            assert_ne!(
                k.tables[bw as usize],
                hf_tables(bw),
                "{} tables should differ from the generated ones",
                bw.hz()
            );
        }
    }
}

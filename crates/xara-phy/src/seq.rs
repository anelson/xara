use crate::rnd::VbRnd;

pub const HF_SEED: f64 = 292.0;
pub const FM_SEED: f64 = 43.0;
pub const WHITENING_LEN: usize = 150_000;
pub const PSK_CARRIERS: usize = 13;
pub const PSK_SYMBOLS: usize = 471;
pub const OFDM_BINS: usize = 128;
pub const OFDM_SYMBOLS: usize = 469;
pub const DATA_DAT_LEN: usize = 65_000;

pub fn whitening(seed: f64) -> Vec<u8> {
    let mut r = VbRnd::stream(seed);
    (0..WHITENING_LEN).map(|_| r.rand_int(1) as u8).collect()
}

pub fn psk_phasor_index() -> Vec<[u8; PSK_SYMBOLS]> {
    let mut r = VbRnd::stream(HF_SEED);
    let mut d = vec![[0u8; PSK_SYMBOLS]; PSK_CARRIERS];
    for ks in [0..411, 411..PSK_SYMBOLS] {
        for row in d.iter_mut() {
            for k in ks.clone() {
                row[k] = r.rand_int(128) as u8;
            }
        }
    }
    d
}

pub fn data_dat() -> Vec<u8> {
    let mut r = VbRnd::stream(HF_SEED);
    (0..DATA_DAT_LEN).map(|_| r.rand_int(128) as u8).collect()
}

pub fn ofdm_phasor_index() -> Vec<[u8; OFDM_SYMBOLS]> {
    let d = data_dat();
    let mut it = d.iter().copied();
    let mut out = vec![[0u8; OFDM_SYMBOLS]; OFDM_BINS];
    for ms in [0..409, 409..OFDM_SYMBOLS] {
        for row in out.iter_mut() {
            for m in ms.clone() {
                row[m] = it.next().unwrap();
            }
        }
    }
    out
}

fn phase_bytes(d: &[u8], len: usize, what: &str) -> Result<(), String> {
    if d.len() != len {
        return Err(format!("{what}: expected {len} bytes, found {}", d.len()));
    }
    match d.iter().position(|&v| v > 128) {
        Some(p) => Err(format!("{what}: phase index out of range at byte {p}")),
        None => Ok(()),
    }
}

/// PSK phase indices read from a user-supplied `Mod2.dat` (VARA HF 4.9.1),
/// consumed in the same order [`psk_phasor_index`] draws them.
pub fn psk_phasor_index_from(d: &[u8]) -> Result<Vec<[u8; PSK_SYMBOLS]>, String> {
    phase_bytes(d, PSK_CARRIERS * PSK_SYMBOLS, "Mod2.dat")?;
    let mut it = d.iter().copied();
    let mut out = vec![[0u8; PSK_SYMBOLS]; PSK_CARRIERS];
    for ks in [0..411, 411..PSK_SYMBOLS] {
        for row in out.iter_mut() {
            for k in ks.clone() {
                row[k] = it.next().unwrap();
            }
        }
    }
    Ok(out)
}

/// OFDM phase indices read from a user-supplied `Mod1.dat` (VARA HF 4.9.1),
/// consumed in the same order [`ofdm_phasor_index`] reads `Data.dat`.
pub fn ofdm_phasor_index_from(d: &[u8]) -> Result<Vec<[u8; OFDM_SYMBOLS]>, String> {
    phase_bytes(d, OFDM_BINS * OFDM_SYMBOLS, "Mod1.dat")?;
    let mut it = d.iter().copied();
    let mut out = vec![[0u8; OFDM_SYMBOLS]; OFDM_BINS];
    for ms in [0..409, 409..OFDM_SYMBOLS] {
        for row in out.iter_mut() {
            for m in ms.clone() {
                row[m] = it.next().unwrap();
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_readers_match_generators() {
        let psk = psk_phasor_index();
        let mut flat = Vec::new();
        for ks in [0..411, 411..PSK_SYMBOLS] {
            for row in &psk {
                flat.extend_from_slice(&row[ks.clone()]);
            }
        }
        assert_eq!(psk_phasor_index_from(&flat), Ok(psk));
        let d = data_dat();
        assert_eq!(ofdm_phasor_index_from(&d[..OFDM_BINS * OFDM_SYMBOLS]), Ok(ofdm_phasor_index()));
    }

    #[test]
    fn file_readers_reject_bad_input() {
        assert!(psk_phasor_index_from(&[0; 10]).is_err());
        assert!(ofdm_phasor_index_from(&data_dat()).is_err(), "Data.dat is the wrong length");
        let mut d = vec![0u8; PSK_CARRIERS * PSK_SYMBOLS];
        d[7] = 129;
        assert!(psk_phasor_index_from(&d).is_err());
    }
}

//! Audio loopback under the 2027 keying (the waveform VARA HF 4.9.1 uses from
//! 2027). The tables come from the user's own VARA installation and are never
//! part of this repository, so these tests only run when `XARA_VARA_DIR`
//! points at one:
//!
//!     XARA_VARA_DIR=/path/to/VARA cargo test -p xara-phy --test keying_2027
//!
//! Setting `XARA_KEYING_NAME=some-name` instead runs the same tests under a
//! named keying generated on the spot (see `xara-keygen`), with no files.
//!
//! They prove that xara talks to itself with the user's tables. They do not
//! prove compatibility with VARA.
//!
//! `cross_keying` checks that the two keyings are really different on air. It
//! is driven by `XARA_KEYING_XFER` (a scratch directory) and
//! `XARA_KEYING_ROLE` (`tx`, `rx-ok` or `rx-fail`), run once per process so
//! one process can transmit under one keying and another receive under the
//! other.

mod common;

use common::{Rng, random_frame};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use xara_phy::call::call_hash;
use xara_phy::dat::hf_tables;
use xara_phy::hf::ctrl::{Codebook, Fmt, Role, leadin_grids, leadin_symbols, synth};
use xara_phy::hf::level::{Bandwidth, Record};
use xara_phy::hf::rx::ctrl::rx_entries;
use xara_phy::hf::rx::receiver::{Event, Receiver};
use xara_phy::hf::tx::Modulator;
use xara_phy::hf::{Keying, install_keying, keying_2027, tables};

/// Install a keying once per test process, before anything else touches the
/// HF tables: the user's VARA files (`XARA_VARA_DIR`), or a named keying
/// generated on the spot (`XARA_KEYING_NAME`). Returns false when neither is
/// set, and the process stays on the keying used up to VARA HF 4.9.0.
fn keyed() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let k = match (std::env::var_os("XARA_VARA_DIR"), std::env::var("XARA_KEYING_NAME")) {
            (Some(dir), _) => Keying::load(Path::new(&dir)).expect("load keying"),
            (None, Ok(name)) => Keying::generate(&name),
            (None, Err(_)) => return false,
        };
        install_keying(k).expect("install keying");
        true
    })
}

fn normalise(x: &[f64]) -> Vec<f64> {
    let rms = (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).sqrt();
    x.iter().map(|v| v / (rms + 1e-12) * 0.12).collect()
}

fn padded(rng: &mut Rng, tx: &[f64], noise: f64) -> Vec<f64> {
    let mut x = vec![0.0; 24000];
    x.extend(normalise(tx));
    x.extend(vec![0.0; 24000]);
    x.iter().map(|v| v + noise * rng.gauss()).collect()
}

fn run(rx: &mut Receiver, x: &[f64]) -> Vec<Event> {
    let mut ev: Vec<Event> = x.chunks(480).flat_map(|b| rx.feed(b)).collect();
    ev.extend(rx.flush());
    ev
}

fn decoded(ev: &[Event]) -> Vec<Vec<u8>> {
    ev.iter()
        .filter_map(|e| match e {
            Event::Frame(f) if f.frame.ok => Some(f.frame.bytes.clone()),
            _ => None,
        })
        .collect()
}

fn codebook(bw: Bandwidth) -> Codebook {
    let mut cb = Codebook::new(call_hash(b"N1CALL"), [call_hash(b"N2CALL"), 0, 0, 0, 0], Role::Caller, bw);
    cb.set_connect_codes(call_hash(b"N2CALL"), Role::Caller);
    cb
}

fn ctrl_audio(rng: &mut Rng, cb: &Codebook, fmt: Fmt, code: u32) -> Vec<f64> {
    let grids = leadin_grids();
    let lead = &grids[fmt.index()][..leadin_symbols(fmt, code, 0) as usize];
    padded(rng, &synth(lead, cb.rx(fmt, code)), 0.02)
}

fn ctrl_heard(bw: Bandwidth, cb: &Codebook, x: &[f64]) -> Vec<(u32, Fmt)> {
    let entries = rx_entries(cb, 0);
    let mut rx = Receiver::new(bw, &[], 1, Some(&entries));
    run(&mut rx, x)
        .iter()
        .filter_map(|e| if let Event::Ctrl(c) = e { Some((c.det.code, c.det.fmt)) } else { None })
        .collect()
}

#[test]
fn user_tables_are_in_use() {
    if !keyed() {
        return;
    }
    assert!(keying_2027());
    for bw in Bandwidth::ALL {
        let (user, generated) = (tables(bw), hf_tables(bw));
        assert_eq!(user.len(), generated.len());
        for (i, (u, g)) in user.iter().zip(&generated).enumerate() {
            assert_eq!((u.chan.len(), u.turbo.len(), u.other.len()), (g.chan.len(), g.turbo.len(), g.other.len()));
            assert_ne!(u.chan, g.chan, "{} record {}: channel interleaver is still the generated one", bw.hz(), i + 1);
        }
    }
}

#[test]
fn every_level_loops_back() {
    if !keyed() {
        return;
    }
    let mut rng = Rng::new(2027);
    let mut fails = Vec::new();
    for bw in Bandwidth::ALL {
        let m = Modulator::new(bw);
        for rec in bw.levels() {
            let frame = random_frame(&mut rng, bw.level(rec).frame_bytes() as usize);
            let tx = m.transmission(rec, &[&frame], &mut |n| rng.below(n));
            let x = padded(&mut rng, &tx, 0.002);
            let mut rx = Receiver::new(bw, &[rec], 1, None);
            let ev = run(&mut rx, &x);
            let txs = ev.iter().filter(|e| matches!(e, Event::Tx(_))).count();
            if decoded(&ev) != [frame.clone()] || txs != 1 {
                fails.push((bw, rec, decoded(&ev).len(), txs));
            }
        }
    }
    assert!(fails.is_empty(), "{fails:?}");
}

#[test]
fn double_frames_loop_back() {
    if !keyed() {
        return;
    }
    let mut rng = Rng::new(2028);
    let mut fails = Vec::new();
    for bw in Bandwidth::ALL {
        let m = Modulator::new(bw);
        for rec in bw.levels() {
            let n = bw.level(rec).frame_bytes() as usize;
            let (f1, f2) = (random_frame(&mut rng, n), random_frame(&mut rng, n));
            let tx = m.transmission(rec, &[&f1, &f2], &mut |n| rng.below(n));
            let x = padded(&mut rng, &tx, 0.002);
            let mut rx = Receiver::new(bw, &[rec], 2, None);
            if decoded(&run(&mut rx, &x)) != [f1.clone(), f2.clone()] {
                fails.push((bw, rec));
            }
        }
    }
    assert!(fails.is_empty(), "{fails:?}");
}

#[test]
fn control_frames_loop_back() {
    if !keyed() {
        return;
    }
    let mut rng = Rng::new(2029);
    for bw in Bandwidth::ALL {
        let cb = codebook(bw);
        for (fmt, code) in [(Fmt::F0, 1), (Fmt::F1, 7), (Fmt::F2, 49), (Fmt::F0, 12), (Fmt::F2, 5), (Fmt::F0, 50)] {
            let x = ctrl_audio(&mut rng, &cb, fmt, code);
            assert_eq!(ctrl_heard(bw, &cb, &x), [(code, fmt)], "{bw:?} fmt {fmt} code {code}");
        }
    }
}

const XFER_CTRL: (Fmt, u32) = (Fmt::F0, 1);

fn xfer_cases(bw: Bandwidth) -> Vec<Record> {
    let top = bw.top_level().get();
    [1, top / 2, top].into_iter().filter_map(Record::new).collect()
}

fn xfer_path(dir: &Path, bw: Bandwidth, what: &str) -> PathBuf {
    dir.join(format!("{}-{what}.f64", bw.hz()))
}

fn put(p: &Path, x: &[f64]) {
    std::fs::write(p, x.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
}

fn get(p: &Path) -> Vec<f64> {
    let b = std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    b.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
}

#[test]
fn cross_keying() {
    let (Some(dir), Ok(role)) = (std::env::var_os("XARA_KEYING_XFER"), std::env::var("XARA_KEYING_ROLE")) else {
        return;
    };
    let dir = PathBuf::from(dir);
    let active = keyed();
    eprintln!("cross_keying: role {role}, 2027 keying {}", if active { "on" } else { "off" });
    for bw in Bandwidth::ALL {
        // The frames depend only on this seed, so both processes agree on them.
        let mut rng = Rng::new(u64::from(bw.hz()));
        let cb = codebook(bw);
        let m = Modulator::new(bw);
        for rec in xfer_cases(bw) {
            let frame = random_frame(&mut rng, bw.level(rec).frame_bytes() as usize);
            let p = xfer_path(&dir, bw, &format!("L{rec}"));
            match role.as_str() {
                "tx" => {
                    let mut noise = Rng::new(99);
                    let tx = m.transmission(rec, &[&frame], &mut |n| noise.below(n));
                    put(&p, &padded(&mut noise, &tx, 0.002));
                }
                "rx-ok" | "rx-fail" => {
                    let mut rx = Receiver::new(bw, &[rec], 1, None);
                    let got = decoded(&run(&mut rx, &get(&p)));
                    match role.as_str() {
                        "rx-ok" => assert_eq!(got, std::slice::from_ref(&frame), "{} L{rec}", bw.hz()),
                        _ => assert!(got.is_empty(), "{} L{rec} decoded across keyings", bw.hz()),
                    }
                }
                r => panic!("unknown XARA_KEYING_ROLE {r}"),
            }
        }
        let (fmt, code) = XFER_CTRL;
        let p = xfer_path(&dir, bw, "ctrl");
        match role.as_str() {
            "tx" => put(&p, &ctrl_audio(&mut Rng::new(98), &cb, fmt, code)),
            "rx-ok" => assert_eq!(ctrl_heard(bw, &cb, &get(&p)), [(code, fmt)], "{} ctrl", bw.hz()),
            _ => {
                assert!(!ctrl_heard(bw, &cb, &get(&p)).contains(&(code, fmt)), "{} ctrl heard across keyings", bw.hz())
            }
        }
    }
}

/// The monitor's blind path: recover the generator state from an overheard
/// control code and turn it back into the call hash that keyed it. Under the
/// 2027 keying this only works if the seed offset is undone.
#[test]
fn blind_recovery_finds_the_key() {
    use xara_phy::hf::rx::blind::{Hypothesis, Key, hypotheses, search_states};
    if !keyed() {
        return;
    }
    let observe = |w: &[xara_phy::hf::ctrl::Symbol]| -> Vec<u32> {
        std::iter::once(40).chain(w.iter().map(|s| s[0])).chain(std::iter::once(60)).collect()
    };
    let bw = Bandwidth::Hz2300;
    let (a, c) = (call_hash(b"N1CALL"), call_hash(b"N0CALL"));
    let ans = Codebook::new(a, [0; 5], Role::Answerer, bw);
    let s = search_states(&observe(ans.tx(Fmt::F1, 18)), Fmt::F1, 2, 11)[0].state;
    let want = Hypothesis { code: 18, sender: Role::Answerer, key: Key::Answerer(a) };
    assert!(hypotheses(s, Fmt::F1, 2, bw).contains(&want), "session code");
    let mut cal = Codebook::new(a, [0; 5], Role::Caller, bw);
    cal.set_connect_codes(c, Role::Caller);
    for code in [49, 50] {
        let s = search_states(&observe(cal.tx(Fmt::F0, code)), Fmt::F0, 2, 18)[0].state;
        let want = Hypothesis { code, sender: Role::Caller, key: Key::Caller(c) };
        assert!(hypotheses(s, Fmt::F0, 2, bw).contains(&want), "connect code {code}");
    }
}

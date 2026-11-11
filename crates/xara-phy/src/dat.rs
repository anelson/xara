use crate::crc::crc16_genibus;
use crate::fm::level::{FmBandwidth, Layout, Record};
use crate::hf::level::{Bandwidth, FSK_PILOTS, Level, Waveform};
use crate::rnd::{Draw, VbRnd};

const HF_PP: u32 = crate::hf::ofdm::PILOT_PERIOD;
const FM_PP: u32 = crate::fm::level::PILOT_PERIOD;

pub const TABLE_SEED: f64 = 43.0;
pub const FM_RECORD16_SEED: f64 = 77.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordTables {
    pub chan: Vec<u32>,
    pub turbo: Vec<u32>,
    pub other: Vec<u16>,
}

pub(crate) fn marker_map(other: &[u16], nc: usize, markers: usize, nsym: usize) -> Vec<Vec<bool>> {
    other[nc * markers..].chunks(nsym).take(nc).map(|r| r.iter().map(|&v| v != 0).collect()).collect()
}

fn perm(r: &mut impl Draw, n: u32) -> Vec<u32> {
    let mut used = vec![false; n as usize];
    let mut out = Vec::with_capacity(n as usize);
    while out.len() < n as usize {
        let v = r.draw(n);
        if !std::mem::replace(&mut used[v as usize], true) {
            out.push(v);
        }
    }
    out
}

fn fsk_other(r: &mut impl Draw, l: &Level) -> Vec<u16> {
    let Waveform::Fsk { symbols: s, first, last, step, .. } = l.waveform else { unreachable!() };
    let nb = last - first + 1;
    let m = if step > 1 { nb / step + 1 } else { nb };
    let mut out: Vec<u16> =
        (0..s - 1).map(|i| (first + u32::from(step > 1 && i % 2 == 0) + step * r.draw(m)) as u16).collect();
    let inc = (s - 2) as f32 / FSK_PILOTS as f32;
    let mut acc = 1.0f32;
    let rows: Vec<u32> = (0..FSK_PILOTS)
        .map(|_| {
            acc += inc;
            acc as u32
        })
        .collect();
    out.extend(rows.iter().map(|&x| x as u16));
    let scale = (1u32 << l.bps) / 8;
    let mut pilot = vec![None; (s - 1) as usize];
    for &row in &rows {
        pilot[(row - 2) as usize] = Some(scale * r.draw(8));
    }
    for p in pilot {
        out.extend(match p {
            Some(v) => [1, v as u16],
            None => [0, 0],
        });
    }
    out
}

fn marker_rows(nc: u32, markers: u32, mut next: impl FnMut(u32) -> u32, gap: u32) -> Vec<Vec<u32>> {
    (0..nc)
        .map(|c| {
            let mut row: Vec<u32> = Vec::with_capacity(markers as usize);
            while row.len() < markers as usize {
                let v = next(c);
                if row.iter().all(|&w| v.abs_diff(w) > gap) {
                    row.push(v);
                }
            }
            row
        })
        .collect()
}

fn carrier_other(rows: Vec<Vec<u32>>, spf: u32) -> Vec<u16> {
    let mut out: Vec<u16> = rows.iter().flatten().map(|&v| v as u16).collect();
    for row in &rows {
        let mut m = vec![0u16; (spf - 1) as usize];
        for &v in row {
            m[(v - 2) as usize] = 1;
        }
        out.extend(m);
    }
    out
}

fn psk_other(r: &mut impl Draw, l: &Level) -> Vec<u16> {
    let (spf, first) = match l.waveform {
        Waveform::Ofdm { spf, first, .. } => (spf, first),
        _ => (l.spf(), 0),
    };
    let qam = l.is_qam();
    let rows = marker_rows(
        l.nc(),
        l.markers(),
        |c| if qam { 3 + (first + c) % 2 + HF_PP * r.draw((spf - 3) / HF_PP) } else { 5 + r.draw(spf - 8) },
        0,
    );
    carrier_other(rows, spf)
}

fn fm_other(r: &mut VbRnd, f: &Layout) -> Vec<u16> {
    let gap = if f.spf < 50 { 0 } else { 2 };
    let rows = marker_rows(
        f.nc,
        f.markers,
        |c| {
            if f.bps > 2 {
                3 + (f.first + f.step * c) % 2 + FM_PP * r.draw((f.spf - 3) / FM_PP)
            } else {
                3 + r.draw(f.spf - 2)
            }
        },
        gap,
    );
    carrier_other(rows, f.spf)
}

pub fn hf_tables(bw: Bandwidth) -> Vec<RecordTables> {
    hf_tables_with(bw, &mut VbRnd::stream(TABLE_SEED))
}

/// Build a full set of tables for `bw` from any random source. With the VARA
/// generator and seed this is the keying used up to VARA HF 4.9.0; with
/// another source it is a valid keying of the user's own.
pub fn hf_tables_with(bw: Bandwidth, r: &mut impl Draw) -> Vec<RecordTables> {
    bw.table()
        .iter()
        .map(|l| {
            let chan = perm(r, l.n());
            let turbo = perm(r, l.k);
            let other = if l.is_fsk() { fsk_other(r, l) } else { psk_other(r, l) };
            debug_assert_eq!(other.len() as u32, l.other_len());
            RecordTables { chan, turbo, other }
        })
        .collect()
}

pub fn fm_tables(bw: FmBandwidth) -> Vec<RecordTables> {
    let mut r = VbRnd::stream(TABLE_SEED);
    bw.layouts()
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if i == Record::Broadcast.index() {
                r = VbRnd::stream(FM_RECORD16_SEED);
            }
            let chan = perm(&mut r, f.n());
            let turbo = perm(&mut r, f.k);
            let other = fm_other(&mut r, f);
            RecordTables { chan, turbo, other }
        })
        .collect()
}

fn put_perm(out: &mut Vec<u8>, p: &[u32]) {
    if p.len() > 65536 {
        out.extend(p.iter().flat_map(|v| v.to_be_bytes()[1..].to_vec()));
    } else {
        out.extend(p.iter().flat_map(|&v| (v as u16).to_be_bytes()));
    }
}

/// Order of the two permutations inside each record of a table file.
///
/// Files up to VARA HF 4.9.0 store the channel interleaver first. The files
/// VARA HF 4.9.1 uses from 2027 (`Binary1/2/3.dat`) store the turbo
/// interleaver first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermOrder {
    ChanFirst,
    TurboFirst,
}

pub(crate) fn serialize_ordered(tables: &[RecordTables], order: PermOrder) -> Vec<u8> {
    let mut out = Vec::new();
    for t in tables {
        let (a, b) = match order {
            PermOrder::ChanFirst => (&t.chan, &t.turbo),
            PermOrder::TurboFirst => (&t.turbo, &t.chan),
        };
        put_perm(&mut out, a);
        put_perm(&mut out, b);
        out.extend(t.other.iter().flat_map(|v| v.to_be_bytes()));
    }
    let crc = crc16_genibus(&out[..out.len() - 1]);
    out.extend(crc.to_be_bytes());
    out
}

fn serialize(tables: &[RecordTables]) -> Vec<u8> {
    serialize_ordered(tables, PermOrder::ChanFirst)
}

fn take_perm(body: &[u8], pos: &mut usize, n: u32, what: &str) -> Result<Vec<u32>, String> {
    let width = if n > 65536 { 3 } else { 2 };
    let end = *pos + n as usize * width;
    let raw = body.get(*pos..end).ok_or_else(|| format!("{what}: file too short"))?;
    *pos = end;
    let p: Vec<u32> = raw.chunks_exact(width).map(|c| c.iter().fold(0u32, |a, &b| a << 8 | u32::from(b))).collect();
    let mut seen = vec![false; n as usize];
    for &v in &p {
        match seen.get_mut(v as usize) {
            Some(s) if !*s => *s = true,
            _ => return Err(format!("{what}: not a permutation of 0..{n}")),
        }
    }
    Ok(p)
}

/// Parse a VARA HF table file supplied by the user instead of generating the
/// tables. The file must have exactly the layout [`hf_dat`] produces (apart
/// from `order`), a valid checksum, and valid permutations in every record.
pub fn hf_tables_from_dat(bw: Bandwidth, dat: &[u8], order: PermOrder) -> Result<Vec<RecordTables>, String> {
    let n = dat.len().checked_sub(2).filter(|&n| n > 0).ok_or("file too short")?;
    let (body, crc) = dat.split_at(n);
    if crc16_genibus(&body[..n - 1]).to_be_bytes().as_slice() != crc {
        return Err("checksum mismatch".into());
    }
    let mut pos = 0;
    let mut out = Vec::new();
    for (i, l) in bw.table().iter().enumerate() {
        let what = |s: &str| format!("record {} {s}", i + 1);
        let (chan, turbo) = match order {
            PermOrder::ChanFirst => {
                let chan = take_perm(body, &mut pos, l.n(), &what("chan"))?;
                let turbo = take_perm(body, &mut pos, l.k, &what("turbo"))?;
                (chan, turbo)
            }
            PermOrder::TurboFirst => {
                let turbo = take_perm(body, &mut pos, l.k, &what("turbo"))?;
                let chan = take_perm(body, &mut pos, l.n(), &what("chan"))?;
                (chan, turbo)
            }
        };
        let end = pos + 2 * l.other_len() as usize;
        let raw = body.get(pos..end).ok_or_else(|| what("other: file too short"))?;
        pos = end;
        let other = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        out.push(RecordTables { chan, turbo, other });
    }
    if pos != body.len() {
        return Err(format!("{} unexpected trailing bytes", body.len() - pos));
    }
    Ok(out)
}

pub fn hf_dat(bw: Bandwidth) -> Vec<u8> {
    serialize(&hf_tables(bw))
}

pub fn fm_dat(bw: FmBandwidth) -> Vec<u8> {
    serialize(&fm_tables(bw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trips_both_orders() {
        for bw in Bandwidth::ALL {
            let t = hf_tables(bw);
            for order in [PermOrder::ChanFirst, PermOrder::TurboFirst] {
                let dat = serialize_ordered(&t, order);
                assert_eq!(hf_tables_from_dat(bw, &dat, order).as_ref(), Ok(&t), "{} {order:?}", bw.hz());
            }
            assert_eq!(hf_tables_from_dat(bw, &hf_dat(bw), PermOrder::ChanFirst).as_ref(), Ok(&t));
        }
    }

    #[test]
    fn parse_rejects_bad_files() {
        let bw = Bandwidth::Hz500;
        let good = hf_dat(bw);
        assert!(hf_tables_from_dat(bw, &good, PermOrder::TurboFirst).is_err(), "wrong order");
        assert!(hf_tables_from_dat(bw, &good[..good.len() - 4], PermOrder::ChanFirst).is_err(), "truncated");
        assert!(hf_tables_from_dat(bw, &[], PermOrder::ChanFirst).is_err(), "empty");
        let mut bad = good.clone();
        bad[0] ^= 1;
        assert_eq!(hf_tables_from_dat(bw, &bad, PermOrder::ChanFirst), Err("checksum mismatch".into()));
        assert!(hf_tables_from_dat(Bandwidth::Hz2300, &good, PermOrder::ChanFirst).is_err(), "wrong bandwidth");
    }
}

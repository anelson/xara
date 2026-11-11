const MASK: u32 = 0xFF_FFFF;
const MUL: u32 = 0xFD43FD;
const INC: u32 = 0xC39EC3;
const MUL_INV: u32 = inverse(MUL);

const fn inverse(m: u32) -> u32 {
    let mut x = m;
    let mut i = 0;
    while i < 5 {
        x = x.wrapping_mul(2u32.wrapping_sub(m.wrapping_mul(x)));
        i += 1;
    }
    x & MASK
}

pub const START_STATE: u32 = 0x05_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VbRnd {
    state: u32,
}

impl Default for VbRnd {
    fn default() -> Self {
        Self::new()
    }
}

impl VbRnd {
    pub const fn new() -> Self {
        Self { state: START_STATE }
    }

    pub const fn from_state(state: u32) -> Self {
        Self { state: state & MASK }
    }

    pub fn stream(seed: f64) -> Self {
        let mut r = Self::new();
        r.rnd(-1.0);
        r.randomize(seed);
        r
    }

    pub const fn state(&self) -> u32 {
        self.state
    }

    pub fn rnd(&mut self, x: f32) -> f32 {
        if x < 0.0 {
            let b = x.to_bits();
            self.state = b.wrapping_add(b >> 24) & MASK;
        }
        if x != 0.0 {
            self.advance();
        }
        self.value()
    }

    pub fn next_f32(&mut self) -> f32 {
        self.advance();
        self.value()
    }

    pub fn randomize(&mut self, d: f64) {
        let hi = (d.to_bits() >> 32) as u32;
        let mid = ((hi & 0xFFFF) ^ (hi >> 16)) & 0xFFFF;
        self.state = (self.state & 0xFF00_00FF) | (mid << 8);
    }

    pub fn draw(&mut self, m: u32) -> u32 {
        (self.next_f32() * m as f32) as u32
    }

    pub fn rand_int(&mut self, n: u32) -> u32 {
        self.draw(n + 1)
    }

    pub fn seed_rnd(&mut self, x: u32) {
        self.seed_rnd_wide(u64::from(x));
    }

    pub(crate) fn seed_rnd_wide(&mut self, x: u64) {
        self.rnd(-1.0);
        self.randomize((x % 32768) as f64);
        for _ in 0..=x / 32768 {
            self.advance();
        }
    }

    pub fn advance(&mut self) {
        self.state = self.state.wrapping_mul(MUL).wrapping_add(INC) & MASK;
    }

    pub fn retreat(&mut self) {
        self.state = self.state.wrapping_sub(INC).wrapping_mul(MUL_INV) & MASK;
    }

    fn value(&self) -> f32 {
        self.state as f32 / 16_777_216.0
    }
}

/// A source of uniform integers for the table generators.
pub trait Draw {
    /// A value in `0..m`.
    fn draw(&mut self, m: u32) -> u32;
}

impl Draw for VbRnd {
    fn draw(&mut self, m: u32) -> u32 {
        VbRnd::draw(self, m)
    }
}

/// SplitMix64, used only to generate user-defined keyings. It is not part of
/// any VARA waveform. The algorithm is fixed: a published keying name must
/// produce the same tables in every version of xara.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Draw for SplitMix64 {
    fn draw(&mut self, m: u32) -> u32 {
        ((u128::from(self.next_u64()) * u128::from(m)) >> 64) as u32
    }
}

/// FNV-1a, 64 bit: turns a keying name into a generator seed.
pub fn fnv1a64(data: &[u8]) -> u64 {
    data.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_and_fnv_reference_values() {
        let mut r = SplitMix64::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(fnv1a64(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xAF63_DC4C_8601_EC8C);
        let mut r = SplitMix64::new(7);
        assert!((0..1000).all(|_| Draw::draw(&mut r, 13) < 13));
    }

    #[test]
    fn spec_examples() {
        let mut r = VbRnd::new();
        assert_eq!(r.next_f32(), 0.705_547_5);
        assert_eq!(r.state(), 0xB49EC3);
        let mut r = VbRnd::new();
        r.rnd(-1.0);
        assert_eq!(r.state(), 0x395886);
        let mut t = VbRnd::stream(292.0);
        let first: Vec<u32> = (0..16).map(|_| t.rand_int(128)).collect();
        assert_eq!(first, [78, 83, 81, 38, 79, 22, 91, 52, 72, 100, 103, 82, 71, 125, 123, 26]);
        assert_eq!(VbRnd::stream(43.0).state(), 0xC04586);
    }

    #[test]
    fn retreat_inverts_advance() {
        let mut r = VbRnd::from_state(0x123456);
        r.advance();
        r.retreat();
        assert_eq!(r.state(), 0x123456);
    }

    #[test]
    fn single_precision_edge() {
        let mut r = VbRnd::from_state(0xF5907A);
        assert_eq!(r.rand_int(5), 5);
    }
}

# contributing

XaraNext is a fork of xara (https://github.com/x0rro/xara) that carries the
waveform keying work: the 2027 keying of VARA HF 4.9.1 and named keyings.

bug fixes to the modem itself belong upstream; send those to xara first.
changes to the keying support, its docs and its tests belong here.

before sending a change, run what CI runs:

    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo test --locked --release --workspace

never add VARA's table files to the repository or to test vectors. the keying
tests read them from a directory you supply (see docs/keying.md).

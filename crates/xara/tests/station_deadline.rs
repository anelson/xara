use xara::station::hf::Station;
use xara::station::{Modem, Options, Report};
use xara_arq::hf::session::{Config, State};
use xara_phy::hf::level::Bandwidth;

const BLOCK: usize = 480;
const MS: i64 = 48;

fn station(bw: Bandwidth, call: &str) -> Station {
    Station::new(Config::new(bw, call), Options::default())
}

fn feed(st: &mut Station, input: &[f64]) -> (Vec<f64>, bool) {
    let mut out = vec![0.0; input.len()];
    let mut keyed = false;
    for (i, o) in input.chunks(BLOCK).zip(out.chunks_mut(BLOCK)) {
        st.process(i, o);
        keyed |= std::iter::from_fn(|| st.pop()).any(|r| r == Report::Ptt(true));
    }
    (out, keyed)
}

fn call(bw: Bandwidth) -> (Station, Vec<f64>, usize) {
    let mut a = station(bw, "N0CALL");
    assert!(a.connect("N0CALL", "N1CALL", &[]));
    let mut played = Vec::new();
    while a.ptt() || played.iter().all(|&v: &f64| v == 0.0) {
        played.extend(feed(&mut a, &[0.0; BLOCK]).0);
        assert!(played.len() < 10 * 48000, "the call never ended");
    }
    let n = played.len();
    (a, played, n)
}

fn answered(bw: Bandwidth, ends_before: i64) -> (State, bool) {
    let (_, call_audio, _) = call(bw);
    let mut b = station(bw, "N1CALL");
    let mut heard = call_audio.clone();
    heard.extend(vec![0.0; 3 * 48000]);
    let (answer, _) = feed(&mut b, &heard);
    let s = answer.iter().position(|&v| v != 0.0).expect("no answer");
    let e = answer.iter().rposition(|&v| v != 0.0).unwrap() + 1;
    let answer = &answer[s..e];

    let (mut a, _, now) = call(bw);
    let deadline = a.session().next_deadline().expect("no deadline after the call");
    let start = (deadline - ends_before) as usize - answer.len();
    assert!(start >= now, "{bw:?}: the answer would start before the call ended");
    let mut x = vec![0.0; start - now];
    x.extend_from_slice(answer);
    x.extend(vec![0.0; 48000]);
    let (_, keyed) = feed(&mut a, &x);
    (a.state(), keyed)
}

#[test]
fn an_answer_ending_before_the_deadline_is_taken() {
    for bw in [Bandwidth::Hz500, Bandwidth::Hz2300] {
        let (state, _) = answered(bw, 5 * MS);
        assert_ne!(state, State::Calling, "{bw:?}: called again over an answer heard 5 ms before the deadline");
    }
}

#[test]
fn an_answer_ending_after_the_deadline_gets_the_call_again() {
    for bw in [Bandwidth::Hz500, Bandwidth::Hz2300] {
        let (state, keyed) = answered(bw, -50 * MS);
        assert!(keyed && state == State::Calling, "{bw:?}: {state:?}, keyed {keyed}: the call was not repeated");
    }
}

//! gmgen in the browser. A bank is built wave by wave so the page can show progress:
//! `begin(set)` (0 = gm.dls set, 1 = gmplus set) → wave count; `step(i)` renders wave `i`; `finish()` writes the DLS
//! and returns its length; the bytes are at `out_ptr()`. Output is byte-identical to `gmgen::bank::GM.build(None)` /
//! `gmgen::plus::PLUS.build(None)` (test `matches_native_build`).
use dls::write::Bank;
use gmgen::bank::{seed_of, Packed, GM};
use gmgen::math::to_i16;
use gmgen::params::Params;
use gmgen::plus::PLUS;
use gmgen::render::render;
use std::cell::RefCell;

struct Job {
    set: &'static Packed,
    bank: Bank,
    params: Vec<Option<Params>>,
    pairs: Vec<(u32, u32)>,
}

thread_local! {
    static JOB: RefCell<Option<Job>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Starts a build of `set` (0 gm, 1 plus); returns the number of waves.
#[unsafe(no_mangle)]
pub extern "C" fn begin(set: u32) -> u32 {
    let set: &'static Packed = if set == 1 { &PLUS } else { &GM };
    let bank = set.skeleton();
    let pairs = bank.pairs.clone();
    let n = bank.waves.len() as u32;
    JOB.with(|j| *j.borrow_mut() = Some(Job { set, bank, params: set.committed(), pairs }));
    n
}

/// Renders wave `i` of the current build (unparametrised waves stay silent, as in `Packed::build`).
#[unsafe(no_mangle)]
pub extern "C" fn step(i: u32) {
    JOB.with(|j| {
        let mut j = j.borrow_mut();
        let job = j.as_mut().expect("begin first");
        let i = i as usize;
        let m = job.set.meta(i).expect("wave meta");
        job.bank.waves[i].samples = match &job.params[i] {
            Some(p) => render(&m, p, seed_of(&job.pairs, i)).iter().map(|x| to_i16(*x)).collect(),
            None => vec![0; m.len],
        };
    });
}

/// Writes the DLS of the current build; returns its length in bytes (at `out_ptr()`).
#[unsafe(no_mangle)]
pub extern "C" fn finish() -> u32 {
    let bytes = JOB.with(|j| dls::write::write(&j.borrow_mut().take().expect("begin first").bank));
    OUT.with(|o| {
        *o.borrow_mut() = bytes;
        o.borrow().len() as u32
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

/// Frees the last output.
#[unsafe(no_mangle)]
pub extern "C" fn release() {
    OUT.with(|o| *o.borrow_mut() = Vec::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built(set: u32) -> Vec<u8> {
        let n = begin(set);
        for i in 0..n {
            step(i);
        }
        let len = finish() as usize;
        OUT.with(|o| o.borrow()[..len].to_vec())
    }

    #[test]
    fn matches_native_build() {
        assert_eq!(built(0), GM.build(None));
    }
}

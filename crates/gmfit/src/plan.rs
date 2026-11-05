//! FFT plans from a per-thread cached planner. A fresh `FftPlanner` per call rebuilt every plan — odd lengths (the
//! bank-speed resample, the roughness bands) as Bluestein plans with large scratch — and 16 fitting threads doing that
//! every evaluation fragmented the heap without bound (one wave's bulk run reached 5.4 GB).
use rustfft::{Fft, FftPlanner};
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static PLANNER: RefCell<FftPlanner<f32>> = RefCell::new(FftPlanner::new());
}

pub fn forward(n: usize) -> Arc<dyn Fft<f32>> {
    PLANNER.with(|p| p.borrow_mut().plan_fft_forward(n))
}

pub fn inverse(n: usize) -> Arc<dyn Fft<f32>> {
    PLANNER.with(|p| p.borrow_mut().plan_fft_inverse(n))
}

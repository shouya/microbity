#![allow(dead_code)]
use cortex_m::peripheral::{DWT, SYST};

struct Clock {
  syst: SYST,
  dwt: DWT,
}

impl Clock {
  pub fn setup(_syst: SYST) -> Self {
    unimplemented!()
  }
}

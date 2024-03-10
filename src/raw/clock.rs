use core::borrow::BorrowMut;

use cortex_m::peripheral::{DWT, SYST};
use microbit::Board;

struct Clock {
  syst: SYST,
  dwt: DWT,
}

impl Clock {
  pub fn setup(board: &Board) -> Self {
    unimplemented!()
  }
}

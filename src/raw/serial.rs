#![allow(unused)]

use core::fmt::Write;

use microbit::{
  board::UartPins,
  hal::uarte::{self, Instance, Parity, UarteRx, UarteTx},
};

static mut TX_BUF: [u8; 1] = [0];
static mut RX_BUF: [u8; 1] = [0];

pub struct Serial<T: Instance> {
  rx: UarteRx<T>,
  tx: UarteTx<T>,
}

impl<T: Instance> Serial<T> {
  pub fn setup(uarte_reg: T, pins: UartPins) -> Self {
    let uarte = uarte::Uarte::new(
      uarte_reg,
      pins.into(),
      Parity::EXCLUDED,
      microbit::hal::uarte::Baudrate::BAUD115200,
    );

    #[allow(static_mut_refs)]
    let (tx, rx) = unsafe { uarte.split(&mut TX_BUF, &mut RX_BUF).unwrap() };
    Self { tx, rx }
  }

  pub fn send_str(&mut self, s: &str) {
    for c in s.chars() {
      self.tx.write_char(c).unwrap();
    }
  }
}

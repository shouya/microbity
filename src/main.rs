// #![deny(unsafe_code)]
#![no_main]
#![no_std]

use cortex_m_rt::entry;
use microbit::Board;

use panic_halt as _;

mod raw_led;

#[entry]
fn main() -> ! {
  let board = Board::take().unwrap();

  raw_led::led_demo(board);

  loop {}
}

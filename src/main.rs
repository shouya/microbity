// #![deny(unsafe_code)]
#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_halt as _;

mod app;
mod raw;

#[entry]
fn main() -> ! {
  app::volume::show_volumne()
}

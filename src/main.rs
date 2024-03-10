// #![deny(unsafe_code)]
#![no_main]
#![no_std]

use cortex_m_rt::entry;
use panic_rtt_target as _;
use rtt_target::rtt_init_print;

mod app;
mod raw;

#[entry]
fn main() -> ! {
  rtt_init_print!();

  // app::playground::playground();
  // app::volume::show_volumne();
  app::temp::measure_temp();
}

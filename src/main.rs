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

  #[cfg(feature = "app_playground")]
  app::playground::playground();
  #[cfg(feature = "app_volume")]
  app::volume::show_volumne();
  #[cfg(feature = "app_temp")]
  app::temp::measure_temp();
  #[cfg(feature = "app_i2c_display")]
  app::i2c_display::run();
  #[cfg(feature = "app_pcm_player")]
  app::pcm_player::play();
  #[cfg(feature = "app_midi_player")]
  app::midi_player::play();
}

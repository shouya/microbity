// #![deny(unsafe_code)]
#![no_main]
#![no_std]

use cortex_m_rt::entry;

// global logger
use defmt_rtt as _;
// panicking behavior
use panic_probe as _;

mod app;
mod raw;

#[entry]
fn main() -> ! {
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
  #[cfg(feature = "app_tone_generator")]
  app::tone_generator::play();
  #[cfg(feature = "app_ble_temp")]
  app::ble_temp::run();
}

use core::{mem, ptr};

use cortex_m::prelude::_embedded_hal_adc_OneShot;
use microbit::{
  gpio::MicrophonePins,
  hal::{prelude::OutputPin, saadc::SaadcConfig, timer, Saadc, Timer},
  pac::SAADC,
  Board,
};

pub struct Microphone {
  pins: MicrophonePins,
  saadc: Saadc,
}

impl Microphone {
  pub fn setup(saadc: SAADC, microphone_pins: MicrophonePins) -> Self {
    let saadc_conf = SaadcConfig::default();
    // so we do not need to take the board
    let mut saadc = Saadc::new(saadc, saadc_conf);

    Self {
      pins: microphone_pins,
      saadc,
    }
  }

  pub fn read(&mut self) -> u16 {
    self.saadc.read(&mut self.pins.mic_in).unwrap() as u16
  }

  pub fn release(self) -> (SAADC, MicrophonePins) {
    let saadc = unsafe { mem::transmute(self.saadc) };
    let pins = self.pins;
    (saadc, pins)
  }
}

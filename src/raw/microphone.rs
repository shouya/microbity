use cortex_m::prelude::_embedded_hal_adc_OneShot;
use microbit::{
  gpio::MicrophonePins,
  hal::{
    gpio::{p0::P0_05, Floating, Input},
    saadc::SaadcConfig,
    Saadc,
  },
  pac::SAADC,
};

pub struct Microphone {
  mic_in: P0_05<Input<Floating>>,
  saadc: Saadc,
}

impl Microphone {
  pub fn setup(saadc: SAADC, microphone_pins: MicrophonePins) -> Self {
    let saadc_conf = SaadcConfig::default();
    let saadc = Saadc::new(saadc, saadc_conf);

    microphone_pins.mic_run.into_open_drain_output(
      microbit::hal::gpio::OpenDrainConfig::Disconnect0HighDrive1,
      microbit::hal::gpio::Level::High,
    );

    let mic_in = microphone_pins.mic_in.into_floating_input();

    Self { mic_in, saadc }
  }

  pub fn read(&mut self) -> u16 {
    self.saadc.read(&mut self.mic_in).unwrap_or_default() as u16
  }

  pub fn sample(&mut self, n: usize) -> u16 {
    let mut avg = 0u16;
    let mut div = 0u16;

    for i in 1..=n {
      let v = self.read();

      let avg_diff = (v as i32 - avg as i32) as i16 / i as i16;
      avg = avg.saturating_add_signed(avg_diff);

      // discard first three samples because the avg is not accurate
      if i > 3 {
        div += (v as i32 - avg as i32).unsigned_abs() as u16;
      }
    }

    div
  }
}

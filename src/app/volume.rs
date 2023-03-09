use microbit::{hal::Timer, Board};

use crate::{raw::LedMatrix, raw::Microphone};

pub fn show_volumne() -> ! {
  let board = Board::take().unwrap();

  let timer = Timer::new(board.TIMER1);
  let mut led = LedMatrix::setup(board.display_pins, timer);
  let mut microphone = Microphone::setup(board.SAADC, board.microphone_pins);

  let mut count: u64 = 0;
  let mut sum: u64 = 0;
  let mut max_value: u16 = 0;
  loop {
    let mic_value = microphone.read();

    // Smoothen the signal as audio comes in waves
    max_value = max_value.max(mic_value);
    sum += mic_value as u64;
    count += 1;

    if count % 100 == 0 {
      let avg = (sum / count) as u16;
      let image = [
        [if max_value > avg + 100 { 1 } else { 0 }; 5],
        [if max_value > avg + 80 { 1 } else { 0 }; 5],
        [if max_value > avg + 60 { 1 } else { 0 }; 5],
        [if max_value > avg + 40 { 1 } else { 0 }; 5],
        [if max_value > avg + 20 { 1 } else { 0 }; 5],
      ];
      led.set_matrix(image);
      led.show(1000);
      max_value = 0;
    }
  }
}

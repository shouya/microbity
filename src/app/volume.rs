use heapless::String;
use microbit::{hal::Timer, Board};

use core::fmt::Write;

use crate::{
  raw::LedMatrix,
  raw::{Microphone, Serial},
};

pub fn show_volumne() -> ! {
  let board = Board::take().unwrap();
  let mut serial = Serial::setup(board.UARTE0, board.uart);
  let mut str_buf: String<64> = String::new();

  let timer = Timer::new(board.TIMER1);
  let mut led = LedMatrix::setup(board.display_pins, timer);
  let mut microphone = Microphone::setup(board.SAADC, board.microphone_pins);

  write!(&mut str_buf, "\r\n\r\n\r\n\r\n\n").unwrap();
  serial.send_str(&str_buf);
  str_buf.clear();

  loop {
    let mic_value = { microphone.sample(10) };

    write!(&mut str_buf, "{mic_value}\r\n").unwrap();
    serial.send_str(&str_buf);
    str_buf.clear();

    let image = [
      [if mic_value > 100 { 1 } else { 0 }; 5],
      [if mic_value > 80 { 1 } else { 0 }; 5],
      [if mic_value > 60 { 1 } else { 0 }; 5],
      [if mic_value > 40 { 1 } else { 0 }; 5],
      [if mic_value > 20 { 1 } else { 0 }; 5],
    ];
    led.set_matrix(image);
    led.show(100);
  }
}

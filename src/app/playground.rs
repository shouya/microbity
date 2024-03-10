use cortex_m::peripheral::syst::SystClkSource::Core;
use cortex_m::peripheral::{DWT, SYST};
use cortex_m::prelude::_embedded_hal_blocking_delay_DelayUs;
use microbit::hal::delay;
use microbit::pac::RTC0;
use microbit::Board;
use rtt_target::{rprintln, rtt_init_print};

pub fn playground() -> ! {
  let mut board = Board::take().unwrap();
  board.SYST.enable_counter();
  board.SYST.set_clock_source(Core);
  board.DCB.enable_trace();
  board.DWT.enable_cycle_counter();

  rprintln!("hello {}", DWT::cycle_counter_enabled());
  rprintln!("hello {}", DWT::cycle_count());

  let mut last_cycle = DWT::cycle_count();
  let mut n = 0;

  let mut delay = delay::Delay::new(board.SYST);
  delay.delay_us(1000u32);

  board.SYST = delay.free();

  loop {
    n = DWT::cycle_count();

    rprintln!("hello {}", DWT::sleep_count());

    last_cycle = n;
  }
}

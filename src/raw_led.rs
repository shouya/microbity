use microbit::{
  gpio::DisplayPins,
  hal::{
    gpio::{Output, Pin, PushPull},
    prelude::OutputPin,
    prelude::_embedded_hal_blocking_delay_DelayUs,
    timer::Instance,
    Timer,
  },
  Board,
};

type Led = Pin<Output<PushPull>>;

#[allow(unused)]
pub fn raw_led_demo(mut board: Board) -> ! {
  let mut timer = Timer::new(board.TIMER0);
  loop {
    board.display_pins.row1.set_high().unwrap();
    board.display_pins.col1.set_low().unwrap();

    timer.delay(100);

    board.display_pins.row1.set_low().unwrap();
    board.display_pins.col1.set_high().unwrap();

    board.display_pins.row2.set_high().unwrap();
    board.display_pins.col2.set_low().unwrap();

    timer.delay(100);

    board.display_pins.row2.set_low().unwrap();
    board.display_pins.col2.set_high().unwrap();
  }
}

#[allow(unused)]
pub fn led_demo(board: Board) {
  light_up(
    1000,
    &mut Timer::new(board.TIMER0),
    board.display_pins,
    [
      [1, 0, 1, 0, 1],
      [0, 1, 0, 1, 0],
      [0, 0, 0, 0, 1],
      [1, 1, 1, 1, 0],
      [0, 0, 1, 0, 0],
    ],
  );
}

#[allow(unused)]
pub fn light_up<T: Instance>(
  ms: u32,
  timer: &mut Timer<T>,
  display_pins: DisplayPins,
  matrix: [[u8; 5]; 5],
) {
  const ROW_LIGHT_UP_US: u32 = 50;
  const ALL_LIGHT_UP_US: u32 = ROW_LIGHT_UP_US * 5;

  let (mut col_pins, mut row_pins) = display_pins.degrade();

  for _ in 0..(ms * 1000 / ALL_LIGHT_UP_US) {
    for r in 0..5 {
      light_up_row(
        ROW_LIGHT_UP_US,
        timer,
        &mut row_pins[r],
        &mut col_pins,
        &matrix[r],
      );
    }
  }
}

fn light_up_row<T: Instance>(
  row_delay_us: u32,
  timer: &mut Timer<T>,
  row_pin: &mut Led,
  col_pins: &mut [Led; 5],
  row: &[u8; 5],
) {
  row_pin.set_high().unwrap();

  for i in 0..5 {
    if row[i] > 0 {
      col_pins[i].set_low().unwrap();
    }
  }

  timer.delay_us(row_delay_us);

  col_pins.iter_mut().for_each(|x| x.set_high().unwrap());
  row_pin.set_low().unwrap();
}

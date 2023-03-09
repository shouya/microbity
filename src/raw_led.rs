use microbit::{
  hal::{
    gpio::{Output, Pin, PushPull},
    prelude::OutputPin,
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
pub fn led_demo(board: Board) -> ! {
  light_up(
    board,
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
pub fn light_up(board: Board, matrix: [[u8; 5]; 5]) -> ! {
  let mut timer = Timer::new(board.TIMER0);
  let (mut col_pins, mut row_pins) = board.display_pins.degrade();

  loop {
    for r in 0..5 {
      light_up_row(
        500,
        &mut timer,
        &mut row_pins[r],
        &mut col_pins,
        &matrix[r],
      );
    }
  }
}

fn light_up_row<T: Instance, U>(
  light_up_cycle: u32,
  timer: &mut Timer<T, U>,
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

  timer.delay(light_up_cycle);

  col_pins.iter_mut().for_each(|x| x.set_high().unwrap());
  row_pin.set_low().unwrap();
}

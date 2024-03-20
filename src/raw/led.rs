#![allow(dead_code)]
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

type LedPin = Pin<Output<PushPull>>;

pub struct LedMatrix<T: Instance> {
  row_pins: [LedPin; 5],
  col_pins: [LedPin; 5],
  timer: Timer<T>,
  matrix: [[u8; 5]; 5],
}

impl<T: Instance> LedMatrix<T> {
  const ROW_LIGHT_UP_US: u32 = 50;

  pub fn setup(display_pins: DisplayPins, timer: Timer<T>) -> Self {
    let (col_pins, row_pins) = display_pins.degrade();
    let matrix = Default::default();

    Self {
      row_pins,
      col_pins,
      timer,
      matrix,
    }
  }

  pub fn show(&mut self, time: u32) {
    for _ in 0..time {
      for r in 0..5 {
        self.light_up_row(r);
      }
    }
  }

  pub fn set_matrix(&mut self, matrix: [[u8; 5]; 5]) {
    self.matrix = matrix;
  }

  pub fn set_cell(&mut self, pos: (usize, usize), val: bool) {
    self.matrix[pos.0][pos.1] = val as u8;
  }

  fn light_up_row(&mut self, r: usize) {
    let row_pin = &mut self.row_pins[r];
    let col_pins = &mut self.col_pins;
    let row = &self.matrix[r];

    row_pin.set_high().unwrap();

    for i in 0..5 {
      if row[i] > 0 {
        col_pins[i].set_low().unwrap();
      }
    }

    self.timer.delay_us(Self::ROW_LIGHT_UP_US);

    col_pins.iter_mut().for_each(|x| x.set_high().unwrap());
    row_pin.set_low().unwrap();
  }
}

#[allow(unused)]
pub fn raw_demo(mut board: Board) -> ! {
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

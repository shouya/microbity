#![allow(clippy::needless_range_loop)]

use core::{
  cell::{Cell, RefCell},
  fmt::Write,
};

use cortex_m::{
  interrupt::{free, CriticalSection, Mutex},
  peripheral::NVIC,
};
use heapless::{String, Vec};
use microbit::{
  display::nonblocking::{BitImage, Display},
  gpio::DisplayPins,
  hal::{
    ppi::{self, ConfigurablePpi, Ppi},
    Temp,
  },
  pac::{interrupt, PPI, TEMP, TIMER0, TIMER1},
  Board,
};
use rtt_target::rprintln;

const NUM_ARRAY: [[[u8; 3]; 5]; 10] = [
  // NUMBER 0
  [[1, 1, 1], [1, 0, 1], [1, 0, 1], [1, 0, 1], [1, 1, 1]],
  // NUMBER 1
  [[0, 1, 0], [1, 1, 0], [0, 1, 0], [0, 1, 0], [1, 1, 1]],
  // NUMBER 2
  [[1, 1, 1], [0, 0, 1], [1, 1, 1], [1, 0, 0], [1, 1, 1]],
  // NUMBER 3
  [[1, 1, 1], [0, 0, 1], [1, 1, 1], [0, 0, 1], [1, 1, 1]],
  // NUMBER 4
  [[1, 0, 1], [1, 0, 1], [1, 1, 1], [0, 0, 1], [0, 0, 1]],
  // NUMBER 5
  [[1, 1, 1], [1, 0, 0], [1, 1, 1], [0, 0, 1], [1, 1, 1]],
  // NUMBER 6
  [[1, 1, 1], [1, 0, 0], [1, 1, 1], [1, 0, 1], [1, 1, 1]],
  // NUMBER 7
  [[1, 1, 1], [0, 0, 1], [0, 1, 0], [0, 1, 0], [0, 1, 0]],
  // NUMBER 8
  [[1, 1, 1], [1, 0, 1], [1, 1, 1], [1, 0, 1], [1, 1, 1]],
  // NUMBER 9
  [[1, 1, 1], [1, 0, 1], [1, 1, 1], [0, 0, 1], [1, 1, 1]],
];

static DISPLAY: Mutex<RefCell<Option<Display<TIMER1>>>> =
  Mutex::new(RefCell::new(None));

static TEMP: Mutex<RefCell<Option<Temp>>> = Mutex::new(RefCell::new(None));
static TIMER0: Mutex<RefCell<Option<TIMER0>>> = Mutex::new(RefCell::new(None));

const BUFFER_SIZE: usize = 10;
static BUFFER: Mutex<RefCell<String<BUFFER_SIZE>>> =
  Mutex::new(RefCell::new(String::new()));
// each character is 3x5, plus a column of space
static FRAMEBUFFER: Mutex<RefCell<Vec<[u8; 5], { 4 * BUFFER_SIZE }>>> =
  Mutex::new(RefCell::new(Vec::new()));
static OFFSET_X: Mutex<Cell<u8>> = Mutex::new(Cell::new(0));

// detect temperature: every 4 secs
const TIMER0_CC0_INTERVAL: u32 = 32768 * 4;
// rotate the display: every 1/4 secs
const TIMER0_CC1_INTERVAL: u32 = 32768 / 4;

pub fn measure_temp() -> ! {
  let mut board = Board::take().unwrap();

  setup_led_display(board.TIMER1, board.display_pins);
  setup_timer(board.TIMER0, board.PPI, &board.TEMP);
  setup_temp(board.TEMP);

  unmask_interrupts(&mut board.NVIC);

  loop {
    cortex_m::asm::wfi();
  }
}

fn setup_led_display(timer: TIMER1, display_pins: DisplayPins) {
  let display = Display::new(timer, display_pins);
  free(|cs| DISPLAY.borrow(cs).replace(Some(display)));
}

pub fn setup_timer(timer: TIMER0, ppi: PPI, temp: &TEMP) {
  timer.tasks_stop.write(|w| w.tasks_stop().set_bit());

  // run timer at 1024Hz (16MHz / 2^9 = 32768Hz)
  // Note: 9 is the largest allowed prescaler value.
  timer.prescaler.write(|w| unsafe { w.prescaler().bits(9) });

  // run in 32-bit mode (default: 16-bit, which corresponds to max cc value of 65535)
  timer.bitmode.write(|w| w.bitmode()._32bit());

  // cc[0]: trigger every 4s, trigger temp read event
  timer.cc[0].write(|w| unsafe { w.bits(TIMER0_CC0_INTERVAL) });

  // cc[1]: trigger every 1/4 s, trigger scroll event
  timer.cc[1].write(|w| unsafe { w.bits(TIMER0_CC1_INTERVAL) });

  // enable interrupt
  timer
    .intenset
    .write(|w| w.compare0().set().compare1().set());

  // programmable peripheral interconnect (PPI)
  // channel[0]: connect TIMER0.EVENTS_COMPARE[0] event to TEMP.TASK_START task
  let mut ppi_parts = ppi::Parts::new(ppi);
  ppi_parts.ppi0.set_event_endpoint(&timer.events_compare[0]);
  ppi_parts.ppi0.set_task_endpoint(&temp.tasks_start);
  ppi_parts.ppi0.enable();

  // start the timer
  timer.tasks_start.write(|w| w.tasks_start().set_bit());

  // put it into the global mutex
  free(|cs| TIMER0.borrow(cs).borrow_mut().replace(timer));
}

pub fn setup_temp(temp: TEMP) {
  // Enable interrupt for TEMP
  temp.intenset.write(|w| w.datardy().set_bit());

  // Start initial measurement
  // temp.tasks_start.write(|w| w.tasks_start().set_bit());

  // Save the TEMP instance into the global mutex
  let temp = Temp::new(temp);
  free(|cs| TEMP.borrow(cs).borrow_mut().replace(temp));
}

pub fn unmask_interrupts(nvic: &mut NVIC) {
  unsafe {
    nvic.set_priority(interrupt::TIMER0, 32);
    nvic.set_priority(interrupt::TIMER1, 48);
    nvic.set_priority(interrupt::TEMP, 64);
    NVIC::unmask(interrupt::TIMER0);
    NVIC::unmask(interrupt::TIMER1);
    NVIC::unmask(interrupt::TEMP);
  }
}

fn update_buffer(cs: &CriticalSection, n: impl core::fmt::Display) {
  let mut buffer = BUFFER.borrow(cs).borrow_mut();
  buffer.clear();
  write!(&mut buffer, "{}", n).unwrap();
}

fn update_framebuffer(cs: &CriticalSection) {
  let buffer = BUFFER.borrow(cs).borrow_mut();
  let mut fb = FRAMEBUFFER.borrow(cs).borrow_mut();
  fb.clear();

  let s = buffer.as_str();
  let blank_column = [0; 5];
  let dot_column = [0, 0, 0, 0, 1];

  for c in s.chars() {
    match c {
      '0'..='9' => {
        let n = c as usize - '0' as usize;
        for x in 0..3 {
          let mut column = [0; 5];
          for y in 0..5 {
            column[y] = NUM_ARRAY[n][y][x];
          }
          fb.push(column).unwrap();
        }
        // add a column of space
        fb.push(blank_column).unwrap();
      }
      '.' => {
        fb.push(blank_column).unwrap();
        fb.push(dot_column).unwrap();
        fb.push(blank_column).unwrap();
      }
      _ => {}
    }
  }

  // add some padding
  for _ in 0..3 {
    fb.push(blank_column).unwrap();
  }

  OFFSET_X.borrow(cs).set(0);
}

fn update_led_display(cs: &CriticalSection) {
  let mut matrix = [[0; 5]; 5];
  let fb = FRAMEBUFFER.borrow(cs).borrow_mut();
  if fb.len() == 0 {
    return;
  }

  let mut offset: usize = OFFSET_X.borrow(cs).get() as usize;
  offset = (offset + 1) % fb.len();
  OFFSET_X.borrow(cs).set(offset as u8);

  for i in 0..5 {
    for j in 0..5 {
      let x: usize = (i + offset) % fb.len();
      let y: usize = j;

      matrix[j][i] = fb[x][y];
    }
  }

  let image = BitImage::new(&matrix);

  DISPLAY
    .borrow(cs)
    .borrow_mut()
    .as_mut()
    .unwrap()
    .show(&image);
}

#[interrupt]
fn TEMP() {
  let reading = free(|cs| {
    let mut borrowed = TEMP.borrow(cs).borrow_mut();
    let temp = borrowed.as_mut().unwrap();
    let val = temp.read().unwrap();
    temp.stop_measurement();
    val
  });

  free(|cs| {
    update_buffer(cs, reading);
    update_framebuffer(cs);
  });

  rprintln!("temp: {}", reading);
}

// update reading to buffer
#[interrupt]
fn TIMER0() {
  free(|cs| {
    let borrowed = TIMER0.borrow(cs).borrow();
    let timer0 = borrowed.as_ref().unwrap();

    if timer0.events_compare[0].read().bits() == 1 {
      timer0.events_compare[0].reset();
      // make the timer periodic
      timer0.cc[0].modify(|r, w| unsafe {
        w.bits(r.bits().wrapping_add(TIMER0_CC0_INTERVAL))
      });
    }

    if timer0.events_compare[1].read().bits() == 1 {
      timer0.events_compare[1].reset();
      // make the timer periodic
      timer0.cc[1].modify(|r, w| unsafe {
        w.bits(r.bits().wrapping_add(TIMER0_CC1_INTERVAL))
      });

      update_led_display(cs);
    }
  });
}

// show the display
#[interrupt]
fn TIMER1() {
  free(|cs| {
    DISPLAY
      .borrow(cs)
      .borrow_mut()
      .as_mut()
      .unwrap()
      .handle_display_event();
  });
}

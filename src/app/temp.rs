use core::{
  borrow::{Borrow, BorrowMut},
  cell::{Cell, RefCell},
  sync::atomic::{AtomicU8, Ordering},
};

use cortex_m::{
  interrupt::{free, Mutex},
  peripheral::NVIC,
};
use microbit::{
  display::nonblocking::{BitImage, Display},
  hal::{
    ppi::{self, ConfigurablePpi, Ppi},
    Temp,
  },
  pac::{interrupt, PPI, TEMP, TIMER0, TIMER2},
  Board,
};
use rtt_target::rprintln;

const NUM_ARRAY: [[[u8; 3]; 5]; 10] = [
  // NUMBER 0
  [[1, 1, 1], [1, 0, 1], [1, 0, 1], [1, 0, 1], [1, 1, 1]],
  // NUMBER 1
  [[1, 1, 0], [0, 1, 0], [0, 1, 0], [0, 1, 0], [1, 1, 1]],
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

static DISPLAY: Mutex<RefCell<Option<Display<TIMER2>>>> =
  Mutex::new(RefCell::new(None));

static TEMP: Mutex<RefCell<Option<Temp>>> = Mutex::new(RefCell::new(None));
static TIMER0: Mutex<RefCell<Option<TIMER0>>> = Mutex::new(RefCell::new(None));

const BUFFER_SIZE: usize = 11;
static BUFFER: Mutex<Cell<[[u8; BUFFER_SIZE]; 5]>> =
  Mutex::new(Cell::new([[0; BUFFER_SIZE]; 5]));
static OFFSET_X: AtomicU8 = AtomicU8::new(0);

// detect temperature: every 1 secs
const TIMER0_CC0_INTERVAL: u32 = 32768;
// rotate the display: every 1/4 secs
const TIMER0_CC1_INTERVAL: u32 = 32768 / 4;

pub fn measure_temp() -> ! {
  let mut board = Board::take().unwrap();

  setup_timer(board.TIMER0, board.PPI, &board.TEMP);
  setup_temp(board.TEMP);

  unmask_interrupts(&mut board.NVIC);

  loop {
    continue;
  }
}

pub fn setup_timer(timer: TIMER0, ppi: PPI, temp: &TEMP) {
  timer.tasks_stop.write(|w| w.tasks_stop().set_bit());

  // run timer at 1024Hz (16MHz / 2^9 = 32768Hz)
  // Note: 9 is the largest allowed prescaler value.
  timer.prescaler.write(|w| unsafe { w.prescaler().bits(9) });

  // cc[0]: trigger every 1s, trigger temp read event
  timer.cc[0].write(|w| unsafe { w.bits(TIMER0_CC0_INTERVAL) });

  // cc[1]: trigger every ~0.1s, trigger scroll event
  timer.cc[1].write(|w| unsafe { w.bits(TIMER0_CC1_INTERVAL) });

  // enable interrupt
  timer
    .intenset
    .write(|w| w.compare0().set().compare1().set());

  // programmable peripheral interconnect
  // PPI channel[0]: connect TIMER0.EVENTS_COMPARE[0] event to TEMP.TASK_START task
  let mut ppi_parts = ppi::Parts::new(ppi);
  ppi_parts.ppi0.set_event_endpoint(&timer.events_compare[0]);
  ppi_parts.ppi0.set_task_endpoint(&temp.tasks_start);
  ppi_parts.ppi0.enable();

  // start the timer event and put it into the global mutex
  timer.tasks_start.write(|w| w.tasks_start().set_bit());
  free(|cs| TIMER0.borrow(cs).borrow_mut().replace(timer));
}

pub fn setup_temp(temp: TEMP) {
  // Enable interrupt for TEMP
  temp.intenset.write(|w| w.datardy().set_bit());

  // save the TEMP instance into the global mutex
  let temp = Temp::new(temp);
  free(|cs| TEMP.borrow(cs).borrow_mut().replace(temp));
}

pub fn unmask_interrupts(nvic: &mut NVIC) {
  unsafe {
    nvic.set_priority(interrupt::TIMER0, 32);
    nvic.set_priority(interrupt::TEMP, 64);
    NVIC::unmask(interrupt::TIMER0);
    NVIC::unmask(interrupt::TEMP);
  }
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
    }
  });
}

// update the display: scrolling
#[interrupt]
fn TIMER1() {
  let mut matrix = [[0; 5]; 5];
  let offset = OFFSET_X.fetch_add(1, Ordering::Relaxed);

  let buffer = free(|cs| BUFFER.borrow(cs).get());

  for i in 0..5 {
    for j in 0..5 {
      let v = buffer[(i + offset as usize) % BUFFER_SIZE][j];
      matrix[i][j] = v;
    }
  }

  let image = BitImage::new(&matrix);

  free(|cs| {
    DISPLAY
      .borrow(cs)
      .borrow_mut()
      .as_mut()
      .unwrap()
      .show(&image);
  });
}

// show the display
#[interrupt]
fn TIMER2() {
  free(|cs| {
    DISPLAY
      .borrow(cs)
      .borrow_mut()
      .as_mut()
      .unwrap()
      .handle_display_event();
  });
}

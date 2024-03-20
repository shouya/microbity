use core::{
  cell::{Cell, OnceCell, RefCell},
  sync::atomic::{AtomicU16, AtomicU32, AtomicUsize, Ordering},
  u16,
};

use cortex_m::{
  asm::{self, delay},
  interrupt::{free, CriticalSection, Mutex},
  peripheral::NVIC,
};
use microbit::{
  hal::{gpio::Level, prelude::OutputPin},
  pac::{interrupt, pwm0::prescaler::PRESCALER_A, GPIOTE, PWM0},
  Board,
};
use rtt_target::rprintln;

// generated using ffmpeg -i bad-apple.webm -ac 1 -ar 2700 -f u8 -t 60 bad-apple.raw
// -ac 1: mono channel
// -ar 2700: sample rate
// -f u8: 8-bit unsigned pcm
// -t 60: 60 seconds (make sure the file is not too large to fit in flash)
const AUDIO_DATA: &[u8] = include_bytes!("../../assets/bad-apple.raw");
// the sample rate of the audio data
const DATA_SAMPLE_RATE: u32 = 16000;
// the speaker's resonance frequency
static TARGET_SAMPLE_RATE: AtomicU32 = AtomicU32::new(16000);

// the prescaler sets the PWM clock frequency.
const PWM_PRESCALER: PRESCALER_A = PRESCALER_A::DIV_1;

// the PWM clock frequency is 16 MHz / (2^PWM_PRESCALER)
const PWM_CLOCK_FREQ: u32 = 1 << (24 - (PWM_PRESCALER as u8));

// each sample will be played REFRESH+1 times. This smooths out the sound.
static PWM_REFRESH: AtomicU32 = AtomicU32::new(3);

// make sure each sample to have duration TARGET_SAMPLE_RATE^-1
// Since: sample duration = (PWM_CLOCK_FREQ / (PWM_COUNTERTOP * (1 + REFRESH)))^-1 = TARGET_SAMPLE_RATE^-1
// Therefore, PWM_COUNTERTOP = PWM_CLOCK_FREQ / (TARGET_SAMPLE_RATE * (1 + REFRESH))
static PWM_COUNTERTOP: AtomicU16 = AtomicU16::new(1); // initialize to an arbitrary value

const GAIN: f32 = 1.0;

static CURSOR: AtomicUsize = AtomicUsize::new(0);

const BUF_LEN: usize = 512;
static BUFFER0: Mutex<RefCell<[u16; BUF_LEN]>> =
  Mutex::new(RefCell::new([0; BUF_LEN]));
static BUFFER1: Mutex<RefCell<[u16; BUF_LEN]>> =
  Mutex::new(RefCell::new([0; BUF_LEN]));

type Pwm = PWM0;
static PWM: Mutex<OnceCell<Pwm>> = Mutex::new(OnceCell::new());
static GPIOTE: Mutex<OnceCell<GPIOTE>> = Mutex::new(OnceCell::new());

#[derive(Clone, Copy)]
#[allow(unused)]
enum ButtonFunction {
  PwmRefresh,
  TargetSampleRate,
}

static BUTTON_FUNCTION: Mutex<Cell<ButtonFunction>> =
  Mutex::new(Cell::new(ButtonFunction::TargetSampleRate));

impl ButtonFunction {
  fn up(&self) {
    match self {
      ButtonFunction::PwmRefresh => PWM_REFRESH.fetch_add(1, Ordering::Relaxed),
      ButtonFunction::TargetSampleRate => {
        TARGET_SAMPLE_RATE.fetch_add(100, Ordering::Relaxed)
      }
    };
  }

  fn down(&self) {
    match self {
      ButtonFunction::PwmRefresh => PWM_REFRESH.fetch_sub(1, Ordering::Relaxed),
      ButtonFunction::TargetSampleRate => {
        TARGET_SAMPLE_RATE.fetch_sub(100, Ordering::Relaxed)
      }
    };
  }
}

pub fn play() -> ! {
  play_sound_data()
}

fn play_sound_data() -> ! {
  let mut board = Board::take().unwrap();

  let speaker_pin = board
    .speaker_pin
    .into_push_pull_output(Level::Low)
    .degrade();

  let pwm = board.PWM0;

  setup_pwm(&pwm, speaker_pin.psel_bits());
  setup_buttons(&board.GPIOTE, board.buttons);

  unsafe { setup_interrupt(&mut board.NVIC) };

  // setup for initial playback
  free(|cs| {
    fill_next_buffer(0, cs);
    fill_next_buffer(1, cs);
  });

  // save pwm for interrupt
  play_seq(0, &pwm);

  // save the peripherals for use in interrupt
  free(|cs| {
    PWM.borrow(cs).set(pwm).unwrap();
    GPIOTE.borrow(cs).set(board.GPIOTE).unwrap();
  });

  loop {
    asm::wfi();
  }
}

// update the pwm countertop if the refresh rate is changed
fn configure_pwm(pwm: &Pwm) {
  let refresh = PWM_REFRESH.load(Ordering::Relaxed);
  let target_sample_rate = TARGET_SAMPLE_RATE.load(Ordering::Relaxed);
  let countertop =
    (PWM_CLOCK_FREQ / (target_sample_rate * (refresh + 1))) as u16;
  PWM_COUNTERTOP.store(countertop, Ordering::Relaxed);

  unsafe {
    // pwm period
    pwm.countertop.write(|w| w.countertop().bits(countertop));

    // each period is repeated REFRESH+1 times
    pwm.seq0.refresh.write(|w| w.bits(refresh));
    pwm.seq1.refresh.write(|w| w.bits(refresh));
  }

  rprintln!(
    "sample rate: {}, refresh {}, counter top: {}",
    target_sample_rate,
    refresh,
    countertop
  );
}

fn setup_buttons(gpiote: &GPIOTE, buttons: microbit::board::Buttons) {
  // enable gpio event for button a
  gpiote.config[0].write(|w| unsafe {
    w.mode()
      .event()
      .psel()
      .bits(buttons.button_a.degrade().pin())
      .polarity()
      .hi_to_lo()
      .outinit()
      .low()
  });

  // enable gpio event for button b
  gpiote.config[1].write(|w| unsafe {
    w.mode()
      .event()
      .psel()
      .bits(buttons.button_b.degrade().pin())
      .polarity()
      .hi_to_lo()
      .outinit()
      .low()
  });

  // enable interrupt
  gpiote.intenset.write(|w| w.in0().set().in1().set());
}

#[allow(unused)]
fn dump_mem(start: u32, end: u32) {
  for i in start..end {
    unsafe {
      let ptr = i as *const u8;
      rprintln!("{:x}: {:x}", i, *ptr);
    }
  }
}

unsafe fn setup_interrupt(nvic: &mut NVIC) {
  nvic.set_priority(interrupt::PWM0, 10);
  NVIC::unmask(interrupt::PWM0);

  nvic.set_priority(interrupt::GPIOTE, 9);
  NVIC::unmask(interrupt::GPIOTE);
}

fn setup_pwm(pwm: &Pwm, speaker_pin: u32) {
  // set pin
  pwm.psel.out[0].write(|w| unsafe { w.bits(speaker_pin) });

  // enable
  pwm.enable.write(|w| w.enable().enabled());

  // mode
  pwm.mode.write(|w| w.updown().up());

  // pwm clock frequency
  pwm
    .prescaler
    .write(|w| w.prescaler().bits(PWM_PRESCALER as u8));

  configure_pwm(pwm);

  // set seq pointer to buffer
  free(|cs| {
    // if the playback goes faster than the cpu can fill in the
    // buffer, the pwm will generate a sequence from garbage. so
    // strictly speaking, the pointer assignments is unsafe. but
    // generally it's much faster to generate the buffer than
    // consuming it. so i'll just keep it this way.
    let buf_0_ptr = BUFFER0.borrow(cs).as_ptr() as u32;
    let buf_1_ptr = BUFFER1.borrow(cs).as_ptr() as u32;
    pwm.seq0.ptr.write(|w| unsafe { w.bits(buf_0_ptr) });
    pwm.seq0.cnt.write(|w| unsafe { w.bits(BUF_LEN as u32) });
    pwm.seq1.ptr.write(|w| unsafe { w.bits(buf_1_ptr) });
    pwm.seq1.cnt.write(|w| unsafe { w.bits(BUF_LEN as u32) });
  });

  // set decode mode to one sample at a time
  pwm
    .decoder
    .write(|w| w.load().common().mode().refresh_count());

  // enable interrupts for end of sequence event
  pwm.intenset.write(|w| w.seqend0().set().seqend1().set());
}

#[interrupt]
fn PWM0() {
  free(|cs| {
    let pwm = PWM.borrow(cs).get().unwrap();
    if pwm.events_seqend[0].read().bits() != 0 {
      pwm.events_seqend[0].write(|w| w.events_seqend().clear_bit());
      play_seq(1, pwm);
      fill_next_buffer(0, cs);
    }

    if pwm.events_seqend[1].read().bits() != 0 {
      pwm.events_seqend[1].write(|w| w.events_seqend().clear_bit());
      play_seq(0, pwm);
      fill_next_buffer(1, cs);
    }
  });
}

#[interrupt]
fn GPIOTE() {
  free(|cs| {
    let gpiote = GPIOTE.borrow(cs).get().unwrap();
    let button_function = BUTTON_FUNCTION.borrow(cs).get();

    // button a pressed
    if gpiote.events_in[0].read().bits() != 0 {
      gpiote.events_in[0].write(|w| w.events_in().clear_bit());
      button_function.up();
      configure_pwm(PWM.borrow(cs).get().unwrap());
    }

    // button b pressed
    if gpiote.events_in[1].read().bits() != 0 {
      gpiote.events_in[1].write(|w| w.events_in().clear_bit());
      button_function.down();
      configure_pwm(PWM.borrow(cs).get().unwrap());
    }
  });
}

fn fill_next_buffer(id: u8, cs: &CriticalSection) {
  let cursor = CURSOR.load(Ordering::Relaxed);
  let buffer = match id {
    0 => BUFFER0.borrow(cs),
    1 => BUFFER1.borrow(cs),
    _ => panic!("invalid id"),
  };

  let mut buffer = buffer.borrow_mut();
  let new_cursor = fill_samples(buffer.as_mut_slice(), AUDIO_DATA, cursor);
  CURSOR.store(new_cursor, Ordering::Relaxed);
}

fn fill_samples(buffer: &mut [u16], data: &[u8], cursor: usize) -> usize {
  // in case the data sample rate is different than the target sample
  // rate, we read the every SAMPLE_STRIDE sample in the data file to
  // get the same sample rate
  let target_sample_rate = TARGET_SAMPLE_RATE.load(Ordering::Relaxed);
  let stride = DATA_SAMPLE_RATE as f32 / target_sample_rate as f32;
  let countertop = PWM_COUNTERTOP.load(Ordering::Relaxed) as usize;
  let pos = |i| (cursor + (stride * i as f32) as usize) % data.len();

  for (i, cell) in buffer.iter_mut().enumerate() {
    let sample = data[pos(i)] as f32 / 255.0;
    let sample = (sample - 0.5) * GAIN + 0.5;
    let sample = (sample * countertop as f32) as u16;
    *cell = sample;
  }

  // return next cursor
  pos(buffer.len())
}

fn play_seq(id: u8, pwm: &Pwm) {
  pwm.tasks_seqstart[id as usize].write(|w| w.tasks_seqstart().trigger());
}

#[allow(unused)]
// square wave
fn naive() -> ! {
  let board = Board::take().unwrap();

  let mut speaker_pin = board
    .speaker_pin
    .into_push_pull_output(Level::Low)
    .degrade();

  loop {
    speaker_pin.set_high().unwrap();
    // 10_000 cycles at 64 MHz ~= 156 us
    // therefore, one period is around 312 us = 3.2 kHz.
    delay(10_000);
    speaker_pin.set_low().unwrap();
    delay(10_000);
  }
}

#[allow(unused)]
fn sleep(n: usize) {
  delay(n as u32);
}

use core::{
  cell::{Cell, OnceCell, RefCell},
  u16,
};

use cortex_m::{
  asm::delay,
  interrupt::{free, CriticalSection, Mutex},
  peripheral::NVIC,
};
use microbit::{
  hal::{gpio::Level, prelude::OutputPin},
  pac::{interrupt, pwm0::prescaler::PRESCALER_A, PWM0},
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
const DATA_SAMPLE_RATE: u32 = 2700;
// the speaker's resonance frequency
const TARGET_SAMPLE_RATE: u32 = 2700;

// each sample will be played REFRESH+1 times. This smooths out the sound.
const REFRESH: u32 = 5;

// the prescaler sets the PWM clock frequency.
const PWM_PRESCALER: PRESCALER_A = PRESCALER_A::DIV_1;

// the PWM clock frequency is 16 MHz / (2^PWM_PRESCALER)
const PWM_CLOCK_FREQ: u32 = 1 << (24 - (PWM_PRESCALER as u8));

// make sure each sample to have duration TARGET_SAMPLE_RATE^-1
// Since: sample duration = (PWM_CLOCK_FREQ / (PWM_COUNTERTOP * (1 + REFRESH)))^-1 = TARGET_SAMPLE_RATE^-1
// Therefore, PWM_COUNTERTOP = PWM_CLOCK_FREQ / (TARGET_SAMPLE_RATE * (1 + REFRESH))
const PWM_COUNTERTOP: u16 =
  (PWM_CLOCK_FREQ / (TARGET_SAMPLE_RATE * (REFRESH + 1))) as u16;

const GAIN: f32 = 1.0;

static CURSOR: Mutex<Cell<usize>> = Mutex::new(Cell::new(0));

const BUF_LEN: usize = 512;
static BUFFER0: Mutex<RefCell<[u16; BUF_LEN]>> =
  Mutex::new(RefCell::new([0; BUF_LEN]));
static BUFFER1: Mutex<RefCell<[u16; BUF_LEN]>> =
  Mutex::new(RefCell::new([0; BUF_LEN]));

type Pwm = PWM0;
static PWM: Mutex<OnceCell<Pwm>> = Mutex::new(OnceCell::new());

pub fn beeper() -> ! {
  play_sound_data()
}

fn play_sound_data() -> ! {
  let mut board = Board::take().unwrap();

  let speaker_pin = board
    .speaker_pin
    .into_push_pull_output(Level::Low)
    .degrade();

  let pwm = board.PWM0;

  rprintln!("parameters: {} {}", PWM_COUNTERTOP, PWM_CLOCK_FREQ);

  setup_pwm(&pwm, speaker_pin.psel_bits());

  unsafe { setup_interrupt(&mut board.NVIC) };

  // setup for initial playback
  free(|cs| {
    fill_next_buffer(0, cs);
    fill_next_buffer(1, cs);
  });

  // save pwm for interrupt
  play_seq(0, &pwm);
  free(|cs| PWM.borrow(cs).set(pwm).unwrap());

  loop {}
}

#[allow(dead_code)]
fn dump_mem(start: u32, end: u32) {
  for i in start..end {
    unsafe {
      let ptr = i as *const u8;
      rprintln!("{:x}: {:x}", i, *ptr);
    }
  }
}

unsafe fn setup_interrupt(nvic: &mut NVIC) {
  nvic.set_priority(interrupt::PWM0, 1);
  NVIC::unmask(interrupt::PWM0);
  // PWM0 is unused
  nvic.set_priority(interrupt::PWM1, 1);
  NVIC::unmask(interrupt::PWM1);
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

  // pwm period frequency = PWM_CLOCK_FREQ / PWM_COUNTERTOP
  pwm
    .countertop
    .write(|w| unsafe { w.countertop().bits(PWM_COUNTERTOP) });

  // each period is repeated REFRESH+1 times
  pwm.seq0.refresh.write(|w| unsafe { w.bits(REFRESH) });
  pwm.seq1.refresh.write(|w| unsafe { w.bits(REFRESH) });

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

fn fill_next_buffer(id: u8, cs: &CriticalSection) {
  let cursor = CURSOR.borrow(cs).get();
  let buffer = match id {
    0 => BUFFER0.borrow(cs),
    1 => BUFFER1.borrow(cs),
    _ => panic!("invalid id"),
  };

  let mut buffer = buffer.borrow_mut();
  let new_cursor = fill_samples(buffer.as_mut_slice(), AUDIO_DATA, cursor);
  CURSOR.borrow(cs).set(new_cursor);
}

// in case the data sample rate is higher than the target sample rate,
// we read the every SAMPLE_STRIDE sample in the data file to get the same sample rate
const SAMPLE_STRIDE: usize = (DATA_SAMPLE_RATE / TARGET_SAMPLE_RATE) as usize;

fn fill_samples(buffer: &mut [u16], data: &[u8], cursor: usize) -> usize {
  let mut cursor = cursor;
  for cell in buffer.iter_mut() {
    let sample = data[cursor] as f32 / 255.0;
    let sample = (sample - 0.5) * GAIN + 0.5;
    let sample = (sample * PWM_COUNTERTOP as f32) as u16;
    *cell = sample;
    cursor = (cursor + SAMPLE_STRIDE) % data.len();
  }

  // return next cursor
  cursor
}

fn play_seq(id: u8, pwm: &Pwm) {
  pwm.tasks_seqstart[id as usize].write(|w| w.tasks_seqstart().trigger());
}

#[allow(dead_code)]
// square wave
fn naive() -> ! {
  let board = Board::take().unwrap();

  let mut speaker_pin = board
    .speaker_pin
    .into_push_pull_output(Level::Low)
    .degrade();

  loop {
    speaker_pin.set_high().unwrap();
    delay(10_000);
    speaker_pin.set_low().unwrap();
    delay(10_000);
  }
}

fn sleep(n: usize) {
  delay(n as u32);
}

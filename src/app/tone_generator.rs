use core::cell::RefCell;

use cortex_m::{
  asm::wfi,
  interrupt::{free, Mutex},
  peripheral::NVIC,
};
use microbit::{
  hal::gpio::{Floating, Input, Level, Output, Pin, PushPull},
  pac::{interrupt, pwm0::prescaler::PRESCALER_A, GPIOTE, PWM0},
  Board,
};
use rtt_target::rprintln;

use micromath::F32Ext;

// the prescaler sets the PWM clock frequency.
const PWM_PRESCALER: PRESCALER_A = PRESCALER_A::DIV_1;
const PWM_CLOCK_FREQ: u32 = 1 << (24 - (PWM_PRESCALER as u8));
const PWM_COUNTER_TOP: u16 = (PWM_CLOCK_FREQ / SAMPLE_RATE) as u16;

const SAMPLE_RATE: u32 = 200000;
const BUFFER_SIZE: usize = 64;

static APP: Mutex<RefCell<Option<App>>> = Mutex::new(RefCell::new(None));

struct Peripherals {
  pwm: PWM0,
  nvic: NVIC,
  speaker_pin: Pin<Output<PushPull>>,
  buttons: [Pin<Input<Floating>>; 2],
  gpiote: GPIOTE,
}

impl Peripherals {
  fn take(board: Board) -> Self {
    let pwm = board.PWM0;
    let nvic = board.NVIC;
    let speaker_pin = board
      .speaker_pin
      .into_push_pull_output(Level::Low)
      .degrade();
    let buttons = [
      board.buttons.button_a.into_floating_input().degrade(),
      board.buttons.button_b.into_floating_input().degrade(),
    ];
    let gpiote = board.GPIOTE;

    Self {
      pwm,
      nvic,
      speaker_pin,
      buttons,
      gpiote,
    }
  }
}

struct NoteGen {
  note: u8,
  volume: u8,
  offset: usize,
  buffers: [[u16; BUFFER_SIZE]; 2],
}

const BASE_FREQ: f32 = 261.62558;
// EXP2_ONE_TWELFTH = 2^(1/12)
const EXP2_ONE_TWELFTH: f32 = 1.0594631;

impl NoteGen {
  fn new() -> Self {
    Self {
      note: 60,
      volume: 127,
      offset: 0,
      buffers: [[0; BUFFER_SIZE]; 2],
    }
  }

  fn freq(&self) -> f32 {
    let x: i32 = self.note as i32 - 60;
    BASE_FREQ * EXP2_ONE_TWELFTH.powi(x)
  }

  // in units of samples
  fn period(&self) -> usize {
    (SAMPLE_RATE as f32 / self.freq()) as usize
  }

  fn fill_buffer(&mut self, buffer_idx: usize) {
    let period = self.period();
    let vol = self.volume as f32 / 127.0;
    let buffer = &mut self.buffers[buffer_idx];

    #[allow(clippy::needless_range_loop)]
    for i in 0..BUFFER_SIZE {
      let phase = ((self.offset + i) % period) as f32 / period as f32;

      let amplitude = sine_waveform(phase);

      buffer[i] = (amplitude * vol * (PWM_COUNTER_TOP as f32)) as u16;
      // rprintln!("{} ({}): sin({}) -> {} ({})", i, phase, x, y, buffer[i]);
    }

    self.offset = (self.offset + BUFFER_SIZE) % period;
  }

  fn set_note(&mut self, note: u8) {
    self.note = note;
    self.offset = 0;

    rprintln!(
      "note: {}, freq: {}, top: {}, period: {}, vol: {}",
      self.note,
      self.freq(),
      PWM_COUNTER_TOP,
      self.period(),
      self.volume as f32 / 127.0
    );
  }
}

struct App {
  // midi key, 60 = middle C
  peripherals: Peripherals,
  note_gen: NoteGen,
}

impl App {
  fn new() -> Self {
    let board = Board::take().unwrap();

    // board
    //   .SYST
    //   .set_clock_source(cortex_m::peripheral::syst::SystClkSource::Core);
    // board.SYST.set_reload(200000);
    // board.SYST.clear_current();
    // board.SYST.enable_counter();

    Self {
      peripherals: Peripherals::take(board),
      note_gen: NoteGen::new(),
    }
  }

  fn setup(&mut self) {
    self.setup_pwm();
    self.setup_buttons();
    self.setup_interrupt();
  }

  fn setup_pwm(&mut self) {
    let pwm = &self.peripherals.pwm;
    let speaker_pin = self.peripherals.speaker_pin.psel_bits();
    pwm.psel.out[0].write(|w| unsafe { w.bits(speaker_pin) });

    pwm.mode.write(|w| w.updown().up());
    pwm
      .prescaler
      .write(|w| w.prescaler().variant(PWM_PRESCALER));
    pwm
      .countertop
      .write(|w| unsafe { w.countertop().bits(PWM_COUNTER_TOP) });

    let buf_len = BUFFER_SIZE as u16;

    let buf_ptr = self.note_gen.buffers[0].as_ptr() as u32;
    pwm.seq0.ptr.write(|w| unsafe { w.bits(buf_ptr) });
    pwm.seq0.cnt.write(|w| unsafe { w.cnt().bits(buf_len) });
    pwm.seq0.refresh.write(|w| w.cnt().continuous());
    pwm.seq0.enddelay.write(|w| unsafe { w.bits(0) });

    let buf_ptr = self.note_gen.buffers[1].as_ptr() as u32;
    pwm.seq1.ptr.write(|w| unsafe { w.bits(buf_ptr) });
    pwm.seq1.cnt.write(|w| unsafe { w.cnt().bits(buf_len) });
    pwm.seq1.refresh.write(|w| w.cnt().continuous());
    pwm.seq1.enddelay.write(|w| unsafe { w.bits(0) });

    pwm
      .decoder
      .write(|w| w.load().common().mode().refresh_count());

    pwm.enable.write(|w| w.enable().enabled());

    pwm.intenset.write(|w| w.seqend0().set().seqend1().set());
  }

  fn setup_buttons(&mut self) {
    let gpiote = &self.peripherals.gpiote;
    let buttons = &self.peripherals.buttons;

    // enable gpio event for button a
    gpiote.config[0].write(|w| unsafe {
      w.mode()
        .event()
        .psel()
        .bits(buttons[0].pin())
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
        .bits(buttons[1].pin())
        .polarity()
        .hi_to_lo()
        .outinit()
        .low()
    });

    // enable interrupt
    gpiote.intenset.write(|w| w.in0().set().in1().set());
  }

  fn setup_interrupt(&mut self) {
    let nvic = &mut self.peripherals.nvic;
    unsafe {
      nvic.set_priority(interrupt::PWM0, 10);
      NVIC::unmask(interrupt::PWM0);

      nvic.set_priority(interrupt::GPIOTE, 1);
      NVIC::unmask(interrupt::GPIOTE);
    }
  }

  fn start(&mut self) {
    self.note_gen.set_note(69);
    self.restart_sequence();
  }

  fn restart_sequence(&mut self) {
    self
      .peripherals
      .pwm
      .tasks_stop
      .write(|w| w.tasks_stop().trigger());

    self.note_gen.fill_buffer(0);
    self.note_gen.fill_buffer(1);

    self.peripherals.pwm.tasks_seqstart[0]
      .write(|w| w.tasks_seqstart().trigger());
  }

  fn handle_pwm_seqend(&mut self) {
    let pwm = &self.peripherals.pwm;

    if pwm.events_seqend[0].read().bits() != 0 {
      // rprintln!("seqend0");
      pwm.events_seqend[0].write(|w| w.events_seqend().clear_bit());
      pwm.tasks_seqstart[1].write(|w| w.tasks_seqstart().trigger());
      self.note_gen.fill_buffer(0);
      return;
    }

    if pwm.events_seqend[1].read().bits() != 0 {
      // rprintln!("seqend1");
      pwm.events_seqend[1].write(|w| w.events_seqend().clear_bit());
      pwm.tasks_seqstart[0].write(|w| w.tasks_seqstart().trigger());
      self.note_gen.fill_buffer(1);
      return;
    }

    rprintln!("Unhandled PWM event");
  }

  fn handle_button_input(&mut self) {
    let gpiote = &self.peripherals.gpiote;

    if gpiote.events_in[0].read().bits() != 0 {
      gpiote.events_in[0].write(|w| w.events_in().clear_bit());

      self.note_gen.set_note(self.note_gen.note.saturating_add(1));
      self.restart_sequence();

      return;
    }

    if gpiote.events_in[1].read().bits() != 0 {
      gpiote.events_in[1].write(|w| w.events_in().clear_bit());

      self.note_gen.set_note(self.note_gen.note.saturating_sub(1));
      self.restart_sequence();

      return;
    }

    rprintln!("Unhandled GPIOTE event");
  }
}

pub fn play() -> ! {
  let app = App::new();

  free(|cs| {
    APP.borrow(cs).replace(Some(app));
  });

  free(|cs| {
    let mut borrowed = APP.borrow(cs).borrow_mut();
    let app = borrowed.as_mut().unwrap();
    app.setup();
    app.start();
  });

  loop {
    wfi();
  }
}

#[interrupt]
fn GPIOTE() {
  free(|cs| {
    let mut borrowed = APP.borrow(cs).borrow_mut();
    let app = borrowed.as_mut().unwrap();
    app.handle_button_input();
  });
}

#[interrupt]
fn PWM0() {
  free(|cs| {
    let mut borrowed = APP.borrow(cs).borrow_mut();
    let app = borrowed.as_mut().unwrap();
    app.handle_pwm_seqend();
  });
}

// input: [0, 1], output: [0, 1]
// allowed because f32::consts doesn't exist in no_std
#[allow(unused)]
#[allow(clippy::approx_constant)]
fn sine_waveform(phase: f32) -> f32 {
  ((2.0 * 3.14159 * phase).sin() + 1.0) * 0.5
}

#[allow(unused)]
fn trig_waveform(phase: f32) -> f32 {
  (0.5 - (phase - 0.5).abs()) * 2.0
}

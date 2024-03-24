use core::cell::{Cell, RefCell};

use cortex_m::{
  asm::wfi,
  interrupt::{free, Mutex},
  peripheral::NVIC,
};
use heapless::Vec;
use microbit::{
  hal::gpio::{Input, Level, Output, Pin, PullUp, PushPull},
  pac::{
    interrupt,
    pwm0::{self, prescaler::PRESCALER_A, RegisterBlock},
    GPIOTE, PWM0, PWM1, PWM2, PWM3, RTC0,
  },
  Board,
};
use micromath::F32Ext;
use midly::{EventIter, TrackEvent, TrackEventKind};
use rtt_target::rprintln;

// http://www.jsbach.net/midi/midi_artoffugue.html
const MIDI_DATA: &[u8] = include_bytes!("../../assets/1080-c01.mid");

static mut BUFFER0: Cell<[u16; 4]> = Cell::new([0; 4]);
static mut BUFFER1: Cell<[u16; 4]> = Cell::new([0; 4]);
static mut BUFFER2: Cell<[u16; 4]> = Cell::new([0; 4]);
static mut BUFFER3: Cell<[u16; 4]> = Cell::new([0; 4]);

// the prescaler sets the PWM clock frequency.
const PWM_PRESCALER: PRESCALER_A = PRESCALER_A::DIV_8;
const PWM_CLOCK_FREQ: u32 = 1 << (24 - (PWM_PRESCALER as u8));

static APP: Mutex<RefCell<Option<AppState>>> = Mutex::new(RefCell::new(None));

struct Peripherals {
  // this field is not used directly. Use Peripherals::pwm(i) to get
  // the pwm register block.
  #[allow(dead_code)]
  pwms: (PWM0, PWM1, PWM2, PWM3),
  rtc: RTC0,
  nvic: NVIC,
  speaker_pin: Pin<Output<PushPull>>,
  gpiote: GPIOTE,
  buttons: [Pin<Input<PullUp>>; 2],
}

enum MidiEvent {
  NoteOn(u8, u8),
  NoteOff(u8),
}

enum NextMidiEvent {
  // channel, event
  Event(u8, MidiEvent),
  Finished,
  Pending,
}

const MAX_TRACKS: usize = 8;

struct Midi {
  // we support at most 8 tracks
  tracks: Vec<EventIter<'static>, MAX_TRACKS>,
  next_event: [Option<TrackEvent<'static>>; MAX_TRACKS],
  ticks: [u32; MAX_TRACKS],
  next_track: Option<(usize, u32)>,
  ticks_per_sec: usize,
}

impl Midi {
  fn load(bytes: &'static [u8]) -> Self {
    use midly::Timing;
    let (header, midly_tracks) = midly::parse(bytes).unwrap();
    let ticks_per_sec = match header.timing {
      Timing::Metrical(n) => n.as_int() as usize,
      Timing::Timecode(fps, n) => (fps.as_int() * n) as usize,
    };

    let mut tracks = Vec::new();
    let mut next_event = [None; MAX_TRACKS];
    for (i, track) in midly_tracks.take(MAX_TRACKS).enumerate() {
      let mut track = track.unwrap();
      next_event[i] = track.next().map(|e| e.unwrap());
      tracks.push(track).unwrap();
    }

    rprintln!("num of tracks: {}", tracks.len());

    let mut this = Self {
      tracks,
      next_event,
      ticks: [0; MAX_TRACKS],
      next_track: None,
      ticks_per_sec,
    };

    this.update_next_track();
    this
  }

  fn ticks_per_sec(&self) -> usize {
    self.ticks_per_sec as usize
  }

  fn update_next_track(&mut self) {
    let mut earliest_tick = u32::MAX;
    let mut earliest_track = None;

    for i in 0..self.tracks.len() {
      if let Some(event) = self.next_event[i].as_ref() {
        let delta = event.delta.as_int();
        let event_tick = self.ticks[i] + delta;
        if event_tick <= earliest_tick {
          earliest_tick = event_tick;
          earliest_track = Some(i);
        }
      }
    }

    self.next_track = earliest_track.map(|i| (i, earliest_tick));
  }

  fn next_event(&mut self) -> Option<TrackEvent<'static>> {
    let (i, tick) = self.next_track.take()?;
    let event = self.next_event[i].take().unwrap();
    self.next_event[i] = self.tracks[i].next().map(|e| e.unwrap());
    self.ticks[i] = tick;
    self.update_next_track();
    Some(event)
  }

  fn next_midi_event(&mut self, tick: u32) -> NextMidiEvent {
    use midly::MidiMessage;

    loop {
      let Some((_next_track, next_tick)) = self.next_track else {
        return NextMidiEvent::Finished;
      };

      if next_tick > tick {
        return NextMidiEvent::Pending;
      }

      let event = self.next_event().unwrap();

      if let TrackEventKind::Midi { message, channel } = event.kind {
        use MidiEvent::{NoteOff, NoteOn};
        let event = match message {
          MidiMessage::NoteOn { key, vel } if vel.as_int() == 0 => {
            NoteOff(key.as_int())
          }
          MidiMessage::NoteOn { key, vel } => {
            NoteOn(key.as_int(), vel.as_int())
          }

          MidiMessage::NoteOff { key, .. } => NoteOff(key.as_int()),
          _ => continue,
        };

        return NextMidiEvent::Event(channel.as_int(), event);
      }
    }
  }
}

struct AppState {
  notes: [Option<u8>; 4],
  midi: Midi,
  peripherals: Peripherals,
  // midi tick
  tick: u32,
}

pub fn play() -> ! {
  let mut app = AppState::new();
  app.setup();
  AppState::start(&mut app);

  free(|cs| {
    APP.borrow(cs).replace(Some(app));
  });

  loop {
    wfi()
  }
}

impl Peripherals {
  fn take(board: Board) -> Self {
    Self {
      pwms: (board.PWM0, board.PWM1, board.PWM2, board.PWM3),
      rtc: board.RTC0,
      nvic: board.NVIC,
      speaker_pin: board
        .speaker_pin
        .into_push_pull_output(Level::Low)
        .degrade(),
      gpiote: board.GPIOTE,
      buttons: [
        board.buttons.button_a.into_pullup_input().degrade(),
        board.buttons.button_b.into_pullup_input().degrade(),
      ],
    }
  }

  fn pwm(&self, i: usize) -> &pwm0::RegisterBlock {
    match i {
      0 => unsafe { &*PWM0::ptr() as &pwm0::RegisterBlock },
      1 => unsafe { &*PWM1::ptr() as &pwm0::RegisterBlock },
      2 => unsafe { &*PWM2::ptr() as &pwm0::RegisterBlock },
      3 => unsafe { &*PWM3::ptr() as &pwm0::RegisterBlock },
      _ => panic!("invalid pwm index"),
    }
  }

  fn pwm_buf(&self, i: usize) -> &'static Cell<[u16; 4]> {
    match i {
      0 => unsafe { &BUFFER0 },
      1 => unsafe { &BUFFER1 },
      2 => unsafe { &BUFFER2 },
      3 => unsafe { &BUFFER3 },
      _ => panic!("invalid pwm index"),
    }
  }

  fn stop_seq(&self, i: usize) {
    self.pwm(i).tasks_stop.write(|w| w.tasks_stop().trigger());
  }
}

impl AppState {
  fn new() -> Self {
    let board = Board::take().unwrap();
    let peripherals = Peripherals::take(board);

    let midi = Midi::load(MIDI_DATA);

    Self {
      notes: [None; 4],
      midi,
      peripherals,
      tick: 0,
    }
  }

  fn setup(&mut self) {
    for i in 0..4 {
      setup_pwm(
        self.peripherals.pwm(i),
        self.peripherals.pwm_buf(i),
        self.peripherals.speaker_pin.psel_bits(),
      );
    }

    setup_timer(&self.peripherals.rtc, self.midi.ticks_per_sec());
    setup_interrupt(&mut self.peripherals.nvic);
    setup_buttons(&self.peripherals.gpiote, &self.peripherals.buttons);
  }

  fn start(&mut self) {
    self
      .peripherals
      .rtc
      .tasks_start
      .write(|w| w.tasks_start().trigger());

    for i in 0..4 {
      self.update_seq(i);
    }
  }

  fn update_seq(&self, i: usize) {
    let buf = self.peripherals.pwm_buf(i);
    let note = self.notes[i];

    match note {
      Some(key) => {
        buf.set(note_to_buf(key));
      }
      None => {
        buf.set(note_off_to_buff());
        self.peripherals.stop_seq(i);
      }
    };
  }

  fn stop(&mut self) {
    self
      .peripherals
      .rtc
      .tasks_stop
      .write(|w| w.tasks_stop().trigger());

    for i in 0..4 {
      self.peripherals.stop_seq(i);
    }
  }

  fn step(&mut self) {
    self.tick += 1;
    loop {
      match self.midi.next_midi_event(self.tick) {
        NextMidiEvent::Event(channel, event) => {
          self.handle_midi_event(channel, event)
        }
        NextMidiEvent::Pending => return,
        NextMidiEvent::Finished => {
          rprintln!("playback finished");
          self.notes = [None; 4];
          self.stop();
          return;
        }
      }
    }
  }

  fn handle_midi_event(&mut self, channel: u8, event: MidiEvent) {
    assert!(channel < 4);

    match event {
      MidiEvent::NoteOn(key, _vel) => {
        self.notes[channel as usize] = Some(key);
      }
      MidiEvent::NoteOff(_key) => {
        self.notes[channel as usize] = None;
      }
    }
  }
}

fn setup_timer(rtc: &RTC0, ticks_per_sec: usize) {
  let prescaler = ((32768 / ticks_per_sec) as u32 - 1) as u16;

  rtc
    .prescaler
    .write(|w| unsafe { w.prescaler().bits(prescaler) });

  rtc.intenset.write(|w| w.tick().set());
}

fn setup_interrupt(nvic: &mut NVIC) {
  unsafe {
    nvic.set_priority(interrupt::RTC0, 10);
    NVIC::unmask(interrupt::RTC0);

    nvic.set_priority(interrupt::GPIOTE, 5);
    NVIC::unmask(interrupt::GPIOTE);
  }
}

fn setup_pwm(pwm: &RegisterBlock, buffer: &Cell<[u16; 4]>, speaker_pin: u32) {
  // set pin
  pwm.psel.out[0].write(|w| unsafe { w.bits(speaker_pin) });

  // mode
  pwm.mode.write(|w| w.updown().up());

  // pwm clock frequency
  pwm
    .prescaler
    .write(|w| w.prescaler().variant(PWM_PRESCALER));

  // set buffer
  let ptr = buffer.as_ptr() as u32;
  pwm.seq0.ptr.write(|w| unsafe { w.bits(ptr) });
  pwm.seq0.cnt.write(|w| unsafe { w.cnt().bits(4) });
  pwm.seq0.refresh.write(|w| unsafe { w.cnt().bits(0x0) });
  pwm.seq0.enddelay.write(|w| unsafe { w.bits(0) });

  // pwm.loop_.write(|w| unsafe { w.bits(0) });

  // repeat a note indefinitely
  pwm.shorts.write(|w| w.loopsdone_seqstart0().enabled());

  // play continuously
  pwm
    .decoder
    .write(|w| w.load().wave_form().mode().next_step());

  // enable
  pwm.enable.write(|w| w.enable().enabled());
}

// 261.625565 Hz = middle C
const BASE_FREQ: f32 = 261.62558;
// EXP2_ONE_TWELFTH = 2^(1/12)
const EXP2_ONE_TWELFTH: f32 = 1.0594631;

fn note_to_buf(key: u8) -> [u16; 4] {
  let x: i32 = key as i32 - 60;
  let freq = BASE_FREQ * EXP2_ONE_TWELFTH.powi(x);

  let countertop = (PWM_CLOCK_FREQ as f32 / freq) as u16;
  let half_duty = countertop / 2;
  rprintln!("key: {}, freq: {}, countertop: {}", key, freq, countertop);
  [half_duty, 0, 0, countertop]
}

fn note_off_to_buff() -> [u16; 4] {
  [0, 0, 0, 3]
}

fn setup_buttons(gpiote: &GPIOTE, buttons: &[Pin<Input<PullUp>>; 2]) {
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

#[interrupt]
fn RTC0() {
  free(|cs| {
    let mut borrowed = APP.borrow(cs).borrow_mut();
    let app = borrowed.as_mut().unwrap();

    app
      .peripherals
      .rtc
      .events_tick
      .write(|w| w.events_tick().clear_bit());

    app.step();
  });
}

#[interrupt]
fn GPIOTE() {
  free(|cs| {
    let mut borrowed = APP.borrow(cs).borrow_mut();
    let app = borrowed.as_mut().unwrap();
    let gpiote = &app.peripherals.gpiote;

    // button a pressed
    if gpiote.events_in[0].read().bits() != 0 {
      gpiote.events_in[0].write(|w| w.events_in().clear_bit());
      *app.notes[0].get_or_insert(60) += 1;
      app.update_seq(0);
    }

    // button b pressed
    if gpiote.events_in[1].read().bits() != 0 {
      gpiote.events_in[1].write(|w| w.events_in().clear_bit());
      *app.notes[0].get_or_insert(60) -= 1;
      app.update_seq(0);
    }
  });
}

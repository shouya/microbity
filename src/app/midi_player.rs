use core::cell::RefCell;

use cortex_m::{
  asm::wfi,
  interrupt::{free, Mutex},
  peripheral::NVIC,
};
use heapless::Vec;
use microbit::{
  hal::gpio::{Level, Output, Pin, PushPull},
  pac::{interrupt, pwm0::prescaler::PRESCALER_A, GPIOTE, PWM0, RTC0},
  Board,
};
use micromath::F32Ext;
use midly::{EventIter, TrackEvent, TrackEventKind};
use rtt_target::rprintln;

// http://www.jsbach.net/midi/midi_artoffugue.html
const MIDI_DATA: &[u8] = include_bytes!("../../assets/1080-c01.mid");

const BUFFER_SIZE: usize = 16;
const SAMPLE_RATE: u32 = 16387;

// the prescaler sets the PWM clock frequency.
const PWM_PRESCALER: PRESCALER_A = PRESCALER_A::DIV_1;
const PWM_CLOCK_FREQ: u32 = 1 << (24 - (PWM_PRESCALER as u8));
const PWM_COUNTERTOP: u16 = (PWM_CLOCK_FREQ / SAMPLE_RATE) as u16;

static APP: Mutex<RefCell<Option<AppState>>> = Mutex::new(RefCell::new(None));

struct Peripherals {
  // this field is not used directly. Use Peripherals::pwm(i) to get
  // the pwm register block.
  #[allow(dead_code)]
  pwm: PWM0,
  rtc: RTC0,
  nvic: NVIC,
  speaker_pin: Pin<Output<PushPull>>,
  gpiote: GPIOTE,
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
    72 * 4
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

      rprintln!("event: {:?}", &event);
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
  buffers: [[u16; BUFFER_SIZE]; 2],
  timestamp: f32,
  waveform: Waveform,
}

pub fn play() -> ! {
  let app = AppState::new();

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
    wfi()
  }
}

impl Peripherals {
  fn take(board: Board) -> Self {
    Self {
      pwm: board.PWM0,
      rtc: board.RTC0,
      nvic: board.NVIC,
      speaker_pin: board.edge.e00.into_push_pull_output(Level::Low).degrade(),
      gpiote: board.GPIOTE,
    }
  }
}

impl AppState {
  fn new() -> Self {
    let board = Board::take().unwrap();
    let peripherals = Peripherals::take(board);

    let midi = Midi::load(MIDI_DATA);

    Self {
      notes: [None; 4],
      buffers: [[0; BUFFER_SIZE]; 2],
      midi,
      peripherals,
      tick: 0,
      waveform: Waveform::Square,
      timestamp: 0.0,
    }
  }

  fn setup(&mut self) {
    self.setup_pwm();
    self.setup_timer();
    self.setup_interrupt();
  }

  fn setup_pwm(&mut self) {
    let speaker_pin = self.peripherals.speaker_pin.psel_bits();
    let pwm = &self.peripherals.pwm;

    // set pin
    pwm.psel.out[0].write(|w| unsafe { w.bits(speaker_pin) });

    // mode
    pwm.mode.write(|w| w.updown().up());

    // pwm clock frequency
    pwm
      .prescaler
      .write(|w| w.prescaler().variant(PWM_PRESCALER));

    // set buffers
    let buf_len = BUFFER_SIZE as u32;

    let buf0 = self.buffers[0].as_ptr() as u32;
    pwm.seq0.ptr.write(|w| unsafe { w.bits(buf0) });
    pwm.seq0.cnt.write(|w| unsafe { w.bits(buf_len) });
    pwm.seq0.refresh.write(|w| w.cnt().continuous());
    pwm.seq0.enddelay.write(|w| unsafe { w.bits(0) });

    let buf1 = self.buffers[1].as_ptr() as u32;
    pwm.seq1.ptr.write(|w| unsafe { w.bits(buf1) });
    pwm.seq1.cnt.write(|w| unsafe { w.bits(buf_len) });
    pwm.seq1.refresh.write(|w| w.cnt().continuous());
    pwm.seq1.enddelay.write(|w| unsafe { w.bits(0) });

    // repeat a note indefinitely
    pwm.shorts.write(|w| w.loopsdone_seqstart0().enabled());

    pwm
      .decoder
      .write(|w| w.load().common().mode().refresh_count());

    let top = PWM_COUNTERTOP as u32;
    pwm.countertop.write(|w| unsafe { w.bits(top) });

    pwm.intenset.write(|w| w.seqend0().set().seqend1().set());

    pwm.enable.write(|w| w.enable().enabled());
  }

  fn setup_timer(&self) {
    let prescaler =
      ((32768.0 / self.midi.ticks_per_sec() as f32).round() - 1.0) as u16;

    self
      .peripherals
      .rtc
      .prescaler
      .write(|w| unsafe { w.prescaler().bits(prescaler) });

    self.peripherals.rtc.intenset.write(|w| w.tick().set());
  }

  fn setup_interrupt(&mut self) {
    unsafe {
      self.peripherals.nvic.set_priority(interrupt::RTC0, 10);
      NVIC::unmask(interrupt::RTC0);

      self.peripherals.nvic.set_priority(interrupt::PWM0, 8);
      NVIC::unmask(interrupt::PWM0);
    }
  }

  fn fill_buffer(&mut self, buffer_idx: usize) {
    let buffer = &mut self.buffers[buffer_idx];
    let dt = 1.0 / SAMPLE_RATE as f32;

    let mut period = 0.0;
    if let Some(highest_note) = self.notes.iter().filter_map(|n| *n).max() {
      period = key_to_period(highest_note);
    }

    #[allow(clippy::needless_range_loop)]
    for i in 0..BUFFER_SIZE {
      if period == 0.0 {
        buffer[i] = 0;
        continue;
      }

      let t = self.timestamp + i as f32 * dt;

      let phase = (t / period).fract();
      let amplitude = self.waveform.sample(phase);
      let v = (amplitude.clamp(-1.0, 1.0) + 1.0) / 1.0;

      buffer[i] = (v * (PWM_COUNTERTOP as f32)) as u16;
    }

    self.timestamp += BUFFER_SIZE as f32 * dt;
  }

  fn start(&mut self) {
    self.fill_buffer(0);
    self.fill_buffer(1);

    self.start_clock();
    self.start_seq(0);
  }

  fn start_seq(&mut self, seq: usize) {
    self.peripherals.pwm.tasks_seqstart[seq]
      .write(|w| w.tasks_seqstart().trigger());
  }

  fn start_clock(&mut self) {
    self
      .peripherals
      .rtc
      .tasks_start
      .write(|w| w.tasks_start().trigger());
  }

  fn stop(&mut self) {
    self
      .peripherals
      .rtc
      .tasks_stop
      .write(|w| w.tasks_stop().trigger());

    self
      .peripherals
      .pwm
      .tasks_stop
      .write(|w| w.tasks_stop().trigger());
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
          break;
        }
      }
    }
  }

  fn handle_midi_event(&mut self, channel: u8, event: MidiEvent) {
    assert!(channel < 4);

    match event {
      MidiEvent::NoteOn(key, _vel) => {
        self.notes[channel as usize] = Some(key);
        rprintln!(
          "note on: {}, period: {} ({}), ctop: {}",
          key,
          key_to_period(key),
          key_to_period(key) * SAMPLE_RATE as f32,
          PWM_COUNTERTOP,
        );
      }
      MidiEvent::NoteOff(key) => {
        self.notes[channel as usize] = None;
        rprintln!("note off: {}", key);
      }
    }
  }

  fn handle_pwm(&mut self) {
    let pwm = &self.peripherals.pwm;

    if pwm.events_seqend[0].read().bits() != 0 {
      pwm.events_seqend[0].write(|w| w.events_seqend().clear_bit());
      self.start_seq(1);
      self.fill_buffer(0);
      return;
    }

    if pwm.events_seqend[1].read().bits() != 0 {
      pwm.events_seqend[1].write(|w| w.events_seqend().clear_bit());
      self.start_seq(0);
      self.fill_buffer(1);
      return;
    }

    rprintln!("Unhandled PWM event");
  }
}

// 261.625565 Hz = middle C
const BASE_PERIOD: f32 = 1.0 / 261.62558;
// EXP2_ONE_TWELFTH = 2^(1/12)
const EXP2_ONE_TWELFTH: f32 = 1.0594631;

fn key_to_period(note: u8) -> f32 {
  let note = 60 - note as i32;
  BASE_PERIOD * EXP2_ONE_TWELFTH.powi(note)
}

enum Waveform {
  Sine,
  Square,
  Triangle,
}

// no_std doesn't have f32::consts::PI
#[allow(clippy::approx_constant)]
const PI: f32 = 3.14159;

impl Waveform {
  // t = [0, 1]
  // output = [-1, 1]
  fn sample(&self, t: f32) -> f32 {
    match self {
      Waveform::Sine => (2.0 * PI * t).sin(),
      Waveform::Square => {
        if t < 0.5 {
          -1.0
        } else {
          1.0
        }
      }
      Waveform::Triangle => {
        if t < 0.5 {
          -4.0 * t - 1.0
        } else {
          4.0 * t + 3.0
        }
      }
    }
  }
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
    }

    // button b pressed
    if gpiote.events_in[1].read().bits() != 0 {
      gpiote.events_in[1].write(|w| w.events_in().clear_bit());
      *app.notes[0].get_or_insert(60) -= 1;
    }
  });
}

#[interrupt]
fn PWM0() {
  free(|cs| {
    let mut borrowed = APP.borrow(cs).borrow_mut();
    let app = borrowed.as_mut().unwrap();
    app.handle_pwm();
  });
}

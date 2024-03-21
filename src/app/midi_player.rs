use core::cell::{Cell, RefCell};

use cortex_m::{
  asm::wfi,
  interrupt::{free, Mutex},
  peripheral::NVIC,
};
use heapless::Vec;
use microbit::{
  board::Buttons,
  hal::gpio::{Disconnected, Floating, Input, Level, Output, Pin, PushPull},
  pac::{
    interrupt,
    pwm0::{self, prescaler::PRESCALER_A, RegisterBlock},
    GPIOTE, PWM0, PWM1, PWM2, PWM3, RTC0,
  },
  Board,
};
use midly::{EventIter, TrackEvent, TrackEventKind, TrackIter};
use rtt_target::rprintln;

// http://www.jsbach.net/midi/midi_artoffugue.html
const MIDI_DATA: &[u8] = include_bytes!("../../assets/1080-c01.mid");

static mut BUFFER0: Cell<[u16; 4]> = Cell::new([0; 4]);
static mut BUFFER1: Cell<[u16; 4]> = Cell::new([0; 4]);
static mut BUFFER2: Cell<[u16; 4]> = Cell::new([0; 4]);
static mut BUFFER3: Cell<[u16; 4]> = Cell::new([0; 4]);

// the prescaler sets the PWM clock frequency.
const PWM_PRESCALER: PRESCALER_A = PRESCALER_A::DIV_32;
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
  buttons: [Pin<Input<Floating>>; 2],
}

enum NextEvent {
  Event(TrackEvent<'static>),
  Finished,
  Pending,
}

const MAX_TRACKS: usize = 3;

struct Tracks {
  // we support at most 8 tracks
  tracks: Vec<EventIter<'static>, MAX_TRACKS>,
  next_event: [Option<TrackEvent<'static>>; MAX_TRACKS],
  ticks: [u32; MAX_TRACKS],
  next_track: Option<(usize, u32)>,
}

impl Tracks {
  fn new(iter: TrackIter<'static>) -> Self {
    let mut tracks = Vec::new();
    let mut next_event = [None; MAX_TRACKS];
    for (i, track) in iter.take(MAX_TRACKS).enumerate() {
      let mut track = track.unwrap();
      next_event[i] = track.next().map(|e| e.unwrap());
      tracks.push(track).unwrap();
    }

    rprintln!("tracks: {}", tracks.len());

    Self {
      tracks,
      next_event,
      ticks: [0; MAX_TRACKS],
      next_track: None,
    }
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

  fn next_midi_event(&mut self, tick: u32) -> NextEvent {
    loop {
      let Some((_next_track, next_tick)) = self.next_track else {
        return NextEvent::Finished;
      };

      if next_tick > tick {
        return NextEvent::Pending;
      }

      let event = self.next_event().unwrap();

      if let TrackEventKind::Midi { .. } = event.kind {
        return NextEvent::Event(event);
      }
    }
  }
}

struct AppState {
  tracks: Tracks,
  notes: [Option<u8>; 4],
  peripherals: Peripherals,
  // midi tick
  tick: u32,
  ticks_per_sec: f32,
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
        board.buttons.button_a.into_floating_input().degrade(),
        board.buttons.button_b.into_floating_input().degrade(),
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

  fn start_seq(&self, i: usize) {
    self.pwm(i).tasks_seqstart[0].write(|w| w.tasks_seqstart().trigger());
  }

  fn stop_seq(&self, i: usize) {
    self.pwm(i).tasks_stop.write(|w| w.tasks_stop().trigger());
  }
}

impl AppState {
  fn new() -> Self {
    let board = Board::take().unwrap();
    let peripherals = Peripherals::take(board);

    let (tracks, ticks_per_sec) = parse_midi();

    Self {
      tracks,
      ticks_per_sec,
      notes: [None; 4],
      peripherals,
      tick: 0,
    }
  }

  fn setup(&mut self) {
    self.update_ticks_per_sec();

    for i in 0..4 {
      setup_pwm(
        self.peripherals.pwm(i),
        self.peripherals.pwm_buf(i),
        self.peripherals.speaker_pin.psel_bits(),
      );
    }

    setup_timer(&self.peripherals.rtc, self.ticks_per_sec);
    setup_interrupt(&mut self.peripherals.nvic);
    setup_buttons(&self.peripherals.gpiote, &self.peripherals.buttons);
  }

  fn start(&mut self) {
    // start ticking
    // self
    //   .peripherals
    //   .rtc
    //   .tasks_start
    //   .write(|w| w.tasks_start().trigger());
  }

  fn update_ticks_per_sec(&mut self) {
    self.ticks_per_sec = 72.0;
    return;

    // while let Some(event) = self.next_event() {
    //   if event.delta > 0 {
    //     // self.pending_events.enqueue(event).unwrap();
    //     break;
    //   }

    //   match event.kind {
    //     TrackEventKind::Meta(MetaMessage::Tempo(n)) => {
    //       rprintln!("tempo change: {}", n.as_int());
    //       self.ticks_per_sec *= 1_000_000.0 / (n.as_int() as f32);
    //     }
    //     TrackEventKind::Meta(_) => {
    //       // ignore other meta
    //     }
    //     TrackEventKind::Midi { .. } => {
    //       // self.pending_events.enqueue(event).unwrap();
    //       break;
    //     }
    //     _ => {
    //       // ignore other event kinds
    //     }
    //   }
    // }
  }

  fn update_seq(&self, i: usize) {
    let buf = self.peripherals.pwm_buf(i);
    let note = self.notes[i];

    match note {
      Some(key) => {
        buf.set(note_to_buf(key));
        self.peripherals.start_seq(i);
      }
      None => {
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
      match self.tracks.next_midi_event(self.tick) {
        NextEvent::Event(event) => self.handle_midi_event(event),
        NextEvent::Pending => return,
        NextEvent::Finished => {
          rprintln!("playback finished");
          self.notes = [None; 4];
          self.stop();
          return;
        }
      }
    }
  }

  fn handle_midi_event(&mut self, event: TrackEvent<'static>) {
    use midly::MidiMessage::{NoteOff, NoteOn};
    use TrackEventKind::Midi;

    rprintln!("midi event: {:?}", event);

    match event.kind {
      Midi {
        message: NoteOff { key, .. },
        channel,
      } => self.handle_note_off(channel.as_int(), key.as_int()),

      Midi {
        message: NoteOn { key, vel },
        channel,
      } if vel.as_int() == 0 => {
        self.handle_note_off(channel.as_int(), key.as_int())
      }
      Midi {
        message: NoteOn { key, .. },
        channel,
      } => self.handle_note_on(channel.as_int(), key.as_int()),
      _ => {}
    }
  }

  fn handle_note_on(&mut self, channel: u8, key: u8) {
    assert!(channel < 4);
    rprintln!("channel: {}, key: {}", channel, key);
    self.notes[channel as usize] = Some(key);
    self.update_seq(channel as usize);
  }

  fn handle_note_off(&mut self, channel: u8, key: u8) {
    assert!(channel < 4);
    rprintln!("channel: {}, key: {}", channel, key);
    self.notes[channel as usize] = None;
    self.update_seq(channel as usize);
  }

  // fn handle_note_on(&mut self, channel: u8, key: u8) {
  //   rprintln!("channel: {}, key: {}", channel, key);
  //   for i in 0..4 {
  //     if self.notes[i].is_none() {
  //       self.notes[i] = Some(key);
  //       self.update_seq(i);
  //       return;
  //     }
  //   }

  //   rprintln!("no free slot for note");
  // }

  // fn handle_note_off(&mut self, channel: u8, key: u8) {
  //   rprintln!("channel: {}, key: {}", channel, key);
  //   for i in 0..4 {
  //     if self.notes[i] == Some(key) {
  //       self.notes[i] = None;
  //       self.update_seq(i);
  //       return;
  //     }
  //   }

  //   rprintln!("note off for non-existent note");
  // }
}

fn setup_timer(rtc: &RTC0, ticks_per_sec: f32) {
  let prescaler = ((32768.0 / ticks_per_sec) as u32 - 1) as u16;

  rtc
    .prescaler
    .write(|w| unsafe { w.prescaler().bits(prescaler) });
  rtc.intenset.write(|w| w.tick().set());
}

fn setup_interrupt(nvic: &mut NVIC) {
  unsafe {
    nvic.set_priority(interrupt::RTC0, 1);

    // nvic.set_priority(interrupt::PWM0, 10);
    // nvic.set_priority(interrupt::PWM1, 11);
    // nvic.set_priority(interrupt::PWM2, 12);
    // nvic.set_priority(interrupt::PWM3, 13);

    // NVIC::unmask(interrupt::PWM0);
    // NVIC::unmask(interrupt::PWM1);
    // NVIC::unmask(interrupt::PWM2);
    // NVIC::unmask(interrupt::PWM3);

    NVIC::unmask(interrupt::RTC0);

    nvic.set_priority(interrupt::GPIOTE, 9);
    NVIC::unmask(interrupt::GPIOTE);
  }
}

fn parse_midi() -> (Tracks, f32) {
  use midly::Timing;
  let (header, tracks) = midly::parse(MIDI_DATA).unwrap();

  // we do not support multi-track formats
  // assert_eq!(header.format, midly::Format::SingleTrack);

  // ticks per second
  let tick_per_sec = match header.timing {
    Timing::Metrical(n) => n.as_int() as f32,
    Timing::Timecode(fps, n) => fps.as_f32() * n as f32,
  };

  let mut tracks = Tracks::new(tracks);
  tracks.update_next_track();
  (tracks, tick_per_sec)
}

fn setup_pwm(pwm: &RegisterBlock, buffer: &Cell<[u16; 4]>, speaker_pin: u32) {
  // set pin
  pwm.psel.out[0].write(|w| unsafe { w.bits(speaker_pin) });

  // enable
  pwm.enable.write(|w| w.enable().enabled());

  // mode
  pwm.mode.write(|w| w.updown().up_and_down());

  // pwm clock frequency
  pwm
    .prescaler
    .write(|w| w.prescaler().bits(PWM_PRESCALER as u8));

  // set buffer
  pwm
    .seq0
    .ptr
    .write(|w| unsafe { w.bits(buffer.as_ptr() as u32) });
  pwm.seq0.cnt.write(|w| unsafe { w.bits(4) });

  // repeat a note indefinitely
  pwm.seq0.refresh.write(|w| unsafe { w.bits(0xffffff) });

  pwm
    .decoder
    .write(|w| w.load().wave_form().mode().refresh_count());

  // initialize to the lowest value
  pwm.countertop.write(|w| unsafe { w.bits(0x3) });

  // enable interrupts for end of sequence event
  // pwm.intenset.write(|w| w.seqend0().set());
}

// 261.625565 Hz = middle C
const BASE_FREQ: f32 = 261.625_58;
const EXP2_ONE_TWELFTH: f32 = 1.059_463_1;

fn note_to_buf(key: u8) -> [u16; 4] {
  // let freq = BASE_FREQ * ((key as f32 - 64.0) / 12.0).exp2(2.0);
  // error: exp2 is not defined because there is no math lib.
  let v = key as i16 - 60;
  let mut multiplier = 1.0;
  if v > 0 {
    for _ in 0..v {
      multiplier *= EXP2_ONE_TWELFTH;
    }
  } else {
    for _ in 0..-v {
      multiplier /= EXP2_ONE_TWELFTH;
    }
  }

  let freq = BASE_FREQ * multiplier;
  let countertop = (PWM_CLOCK_FREQ as f32 / freq) as u16;
  let half_duty = countertop / 2;
  rprintln!("key: {}, freq: {}, countertop: {}", key, freq, countertop);
  [half_duty, half_duty, half_duty, countertop]
}

fn setup_buttons(gpiote: &GPIOTE, buttons: &[Pin<Input<Floating>>; 2]) {
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

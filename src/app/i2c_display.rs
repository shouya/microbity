use cortex_m::asm::delay;
use microbit::{
  hal::{
    twim::{self},
    Twim,
  },
  Board,
};
use panic_rtt_target as _;
use rtt_target::rprintln;

const DISPLAY_ADDR: u8 = 0x3c;

#[link_section = ".data"]
static mut BUFFER: [u8; 1024] = [0x23u8; { 128 * 64 / 8 }];

pub fn run() -> ! {
  let board = Board::take().unwrap();
  let twim_pins = twim::Pins {
    // ring 0
    scl: board.edge.e00.into_floating_input().degrade(),
    sda: board.edge.e01.into_floating_input().degrade(),
  };
  let mut twim = setup_i2c(board.TWIM0, twim_pins);

  rprintln!("initialized i2c");
  initialize_display(&mut twim, DISPLAY_ADDR);
  sleep(10_000);

  rprintln!("initialized display");

  let mut status: [u8; 2] = [0xaa, 0xaa];
  twim.read(DISPLAY_ADDR, &mut status).unwrap();
  rprintln!("status: {:x?}", status);

  loop {
    for b in unsafe { BUFFER.iter_mut() } {
      *b = (*b).wrapping_add(1);
    }
    send_data(&mut twim, DISPLAY_ADDR, unsafe { &BUFFER });
    sleep(30_000_000);
  }
}

fn initialize_display_2<T>(twim: &mut Twim<T>, addr: u8)
where
  T: twim::Instance,
{
  // display off
  send_cmd(twim, addr, [0xae]);
  // set MUX ratio
  send_cmd(twim, addr, [0xa8, 0x3f]);
  // set display offset
  send_cmd(twim, addr, [0xd3, 0x00]);
  // set display start line
  send_cmd(twim, addr, [0x40, 0x00]);
  // set segment remap
  send_cmd(twim, addr, [0xa0]);
  // set COM output scan direction
  send_cmd(twim, addr, [0xc8]);
  // set COM pins hardware configuration
  send_cmd(twim, addr, [0xda, 0x02]);
  // set contrast
  send_cmd(twim, addr, [0x81, 0x7f]);
  // set brightness (Precharge period: 2, Contrast: 0x5f)
  send_cmd(twim, addr, [0xd9, ((0xf & 0x2) << 4) | (0xf & 0x5f)]);
  // disabled entire display on
  send_cmd(twim, addr, [0xa4]);
  // set normal display
  send_cmd(twim, addr, [0xa6]);
  // set osc frequency
  send_cmd(twim, addr, [0xd5, 0x80]);
  // enable charge pump regulator
  send_cmd(twim, addr, [0x8d, 0x14]);
  // display on
  send_cmd(twim, addr, [0xaf]);
}

fn initialize_display<T>(twim: &mut Twim<T>, addr: u8)
where
  T: twim::Instance,
{
  // display off
  send_cmd(twim, addr, [0xae]);
  // display clock divide ratio/oscillator frequency
  send_cmd(twim, addr, [0xd5, (0xf & 0x8) << 4 | 0xf & 0x0]);
  // multiplex ratio
  send_cmd(twim, addr, [0xa8, 64 - 1]); // DisplaySize128x64::HEIGHT - 1
                                        // display offset
  send_cmd(twim, addr, [0xd3, 0x0]);
  // set start line
  send_cmd(twim, addr, [0x40 | (0x3f & 0x0)]);
  // charge pump
  send_cmd(twim, addr, [0x8d, 0x10 | (1 << 2)]);
  // address mode
  send_cmd(twim, addr, [0x20, 1]);
  // configure display size
  send_cmd(twim, addr, [0xda, 0x2 | (1 << 4) | (0 << 5)]);
  // set rotation
  send_cmd(twim, addr, [0xa0 | (true as u8)]); // SegmentRemap(true)
  send_cmd(twim, addr, [0xc0 | ((true as u8) << 3)]); // ReverseComDir(true)
                                                      // set brightness
  send_cmd(twim, addr, [0xd9, ((0xf & 0x2) << 4) | (0xf & 0x5f)]); // Precharge period: 2, Contrast: 0x5f
  send_cmd(twim, addr, [0x81, 0x5f]); // Contrast: 0x5f
                                      // vcomh deselect
  send_cmd(twim, addr, [0xdb, 0b010 << 4]);
  // all on
  send_cmd(twim, addr, [0xa4 | (false as u8)]);
  // invert
  send_cmd(twim, addr, [0xa6 | (false as u8)]);
  // enable scroll
  send_cmd(twim, addr, [0x2e | (false as u8)]);
  // display on
  send_cmd(twim, addr, [0xaf]);
}

fn send_cmd<T, const N: usize>(twim: &mut Twim<T>, addr: u8, cmd: [u8; N])
where
  T: twim::Instance,
{
  assert!(cmd.len() < 8);
  let mut write_buf = [0u8; 8];
  write_buf[1..=cmd.len()].copy_from_slice(&cmd);
  rprintln!("sending command: {:x?}", cmd);
  twim.write(addr, &write_buf[..=cmd.len()]).unwrap();
}

fn send_data<T>(twim: &mut Twim<T>, addr: u8, data: &[u8])
where
  T: twim::Instance,
{
  let mut write_buf = [0u8; 17];
  for chunk in data.chunks(16) {
    write_buf[0] = 0x40; // set the D/C# bit
    write_buf[1..=chunk.len()].copy_from_slice(chunk);
    rprintln!("sending data: {:x?}", chunk);
    twim.write(addr, &write_buf[..=chunk.len()]).unwrap();
  }
}

fn sleep(n: usize) {
  delay(n as u32);
}

pub fn setup_i2c<T>(twim: T, pins: twim::Pins) -> Twim<T>
where
  T: twim::Instance,
{
  let freq = twim::Frequency::K100;
  Twim::new(twim, pins, freq)
}

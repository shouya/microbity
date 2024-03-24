use embassy_executor::{task, Executor, Spawner};
use embassy_time::Timer;
use rtt_target::rprintln;
use static_cell::StaticCell;

static EXECUTOR: StaticCell<Executor> = StaticCell::new();

pub fn run() -> ! {
  let executor = EXECUTOR.init(Executor::new());
  embassy_nrf::init(Default::default());
  executor.run(|spawner| spawner.must_spawn(main(spawner)))
}

#[task]
async fn main(spawner: Spawner) {
  loop {
    Timer::after_millis(500).await;
    rprintln!("Hello, BLE Temp!");
  }
}

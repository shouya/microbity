use core::mem::transmute;

use defmt::{dbg, info};
use embassy_time::{Duration, Timer};
use nrf_softdevice::ble::Connection;
use static_cell::StaticCell;

use embassy_nrf::{self as _}; // time driver

use embassy_nrf::{
  bind_interrupts,
  interrupt::Priority,
  temp::{self, Temp},
};

use nrf_softdevice::{
  self as _,
  ble::{
    advertisement_builder::{
      Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload,
      ServiceList, ServiceUuid16,
    },
    gatt_server, peripheral,
  },
  raw::{self},
  Softdevice,
}; // critical section definition

use embassy_executor::{task, Executor, Spawner};

static EXECUTOR: StaticCell<Executor> = StaticCell::new();
static mut CONNECTION: Option<u16> = None;

pub fn run() -> ! {
  let executor = EXECUTOR.init(Executor::new());
  let mut config = embassy_nrf::config::Config::default();
  config.gpiote_interrupt_priority = Priority::P2;
  config.time_interrupt_priority = Priority::P2;
  embassy_nrf::init(config);

  executor.run(|spawner| spawner.must_spawn(main(spawner)))
}

#[task]
async fn softdevice_task(softdevice: &'static Softdevice) {
  softdevice.run().await;
}

#[nrf_softdevice::gatt_service(uuid = "272F")]
struct TempService {
  #[characteristic(uuid = "2A6E", read, notify)]
  temp: [u8; 5],
}

#[nrf_softdevice::gatt_server]
struct Server {
  temp: TempService,
}

#[allow(clippy::field_reassign_with_default)]
fn setup_softdevice() -> &'static mut Softdevice {
  let mut config = nrf_softdevice::Config::default();
  config.gap_role_count = Some(raw::ble_gap_cfg_role_count_t {
    adv_set_count: 1,
    periph_role_count: 1,
  });
  config.conn_gap = Some(raw::ble_gap_conn_cfg_t {
    conn_count: 1,
    event_length: 24,
  });

  Softdevice::enable(&config)
}

async fn handle_connection(softdevice: &Softdevice, server: &Server) {
  static ADV_DATA: LegacyAdvertisementPayload =
    LegacyAdvertisementBuilder::new()
      .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
      .services_16(ServiceList::Complete, &[ServiceUuid16::HEALTH_THERMOMETER])
      .build();

  // but we can put it in the scan data
  // so the full name is visible once connected
  static SCAN_DATA: LegacyAdvertisementPayload =
    LegacyAdvertisementBuilder::new()
      .full_name("microbity")
      .build();
  let config = peripheral::Config::default();
  let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
    adv_data: &ADV_DATA,
    scan_data: &SCAN_DATA,
  };
  let conn = peripheral::advertise_connectable(softdevice, adv, &config).await;

  let conn = match conn {
    Ok(conn) => conn,
    Err(e) => {
      dbg!(e);
      return;
    }
  };

  info!("Connected");

  gatt_server::run(&conn, server, |e| match e {
    ServerEvent::Temp(temp_e) => match temp_e {
      TempServiceEvent::TempCccdWrite { .. } => {
        unsafe {
          CONNECTION = conn.handle();
        };
      }
    },
  })
  .await;
}

#[task]
async fn monitor_temp(server: &'static Server) {
  let config = embassy_nrf::config::Config::default();
  let peripherals = embassy_nrf::init(config);
  bind_interrupts!(struct Irqs {
    TEMP => temp::InterruptHandler;
  });
  let mut temp = Temp::new(peripherals.TEMP, Irqs);

  loop {
    let readout = temp.read().await.to_bits() as i16;
    let gatt_val = fixed_temp_gatt_value(readout, 2);
    server.temp.temp_set(&gatt_val).unwrap();

    if let Some(handle) = unsafe { CONNECTION.as_ref() } {
      if let Some(conn) = Connection::from_handle(*handle) {
        server.temp.temp_notify(&conn, &gatt_val).unwrap();
      }
    }

    Timer::after(Duration::from_millis(1000)).await;
  }
}

#[task]
async fn main(spawner: Spawner) {
  let softdevice = setup_softdevice();
  let server = Server::new(softdevice).unwrap();

  spawner.spawn(softdevice_task(softdevice)).unwrap();

  server
    .temp
    .temp_set(
      &fixed_temp_gatt_value(-1337, 2), // -13.37
    )
    .unwrap();

  loop {
    info!("Advertising");
    handle_connection(softdevice, &server).await;
  }
}

fn fixed_temp_gatt_value(n: i16, digits: i8) -> [u8; 5] {
  let exponent = unsafe { transmute::<i8, u8>(-digits) };
  let n = unsafe { transmute::<i16, u16>(n) };
  let flags = 0x0; // celsius
  [(n & 0xff) as u8, (n >> 8) as u8, 0, exponent, flags]
}

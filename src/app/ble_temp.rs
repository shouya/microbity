use core::mem::transmute;

use defmt::{dbg, info};
use embassy_time::{Duration, Timer};
use fixed::types::I30F2;
use nrf_softdevice::ble::Connection;
use nrf_softdevice::raw::sd_temp_get;
use nrf_softdevice::RawError;
use static_cell::StaticCell;

use embassy_nrf::{self as _, Peripherals}; // time driver

use embassy_nrf::interrupt::Priority;

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
static SERVER: StaticCell<Server> = StaticCell::new();
static mut CONNECTION: Option<u16> = None;

pub fn run() -> ! {
  let executor = EXECUTOR.init(Executor::new());

  let mut config = embassy_nrf::config::Config::default();
  config.gpiote_interrupt_priority = Priority::P2;
  config.time_interrupt_priority = Priority::P2;
  let peripherals = embassy_nrf::init(config);

  executor.run(|spawner| spawner.must_spawn(main(spawner, peripherals)))
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

// TEMP peripheral was taken by the softdevice
// See: https://infocenter.nordicsemi.com/index.jsp?topic=%2Fsds_s132%2FSDS%2Fs1xx%2Fsd_resource_reqs%2Fhw_block_interrupt_vector.html
#[task]
async fn monitor_temp(server: &'static Server) {
  loop {
    let mut fixed = 0;
    let ret = unsafe { sd_temp_get(&mut fixed) };
    let readout: i32 = (I30F2::from_bits(fixed) * 100).to_num();
    if let Err(e) = RawError::convert(ret) {
      dbg!(e);
      continue;
    };
    let gatt_val = fixed_temp_gatt_value(readout as i16, 2);
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
async fn main(spawner: Spawner, _peripherals: Peripherals) {
  let softdevice = setup_softdevice();
  let server = SERVER.init(Server::new(softdevice).unwrap());

  spawner.spawn(monitor_temp(server)).unwrap();
  spawner.spawn(softdevice_task(softdevice)).unwrap();

  loop {
    info!("Advertising");
    handle_connection(softdevice, server).await;
  }
}

fn fixed_temp_gatt_value(n: i16, digits: i8) -> [u8; 5] {
  let exponent = unsafe { transmute::<i8, u8>(-digits) };
  let n = unsafe { transmute::<i16, u16>(n) };
  let flags = 0x0; // celsius
  [(n & 0xff) as u8, (n >> 8) as u8, 0, exponent, flags]
}

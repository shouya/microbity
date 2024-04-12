use core::{mem, ops::BitAnd};

use defmt::{dbg, info};
use static_cell::StaticCell;

use embassy_nrf::{self as _, interrupt::Priority}; // time driver

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
  temp: [u8; 4],
}

#[nrf_softdevice::gatt_server]
struct Server {
  temp: TempService,
}

#[task]
async fn main(spawner: Spawner) {
  // let config = nrf_softdevice::Config {
  //   clock: Some(raw::nrf_clock_lf_cfg_t {
  //     source: raw::NRF_CLOCK_LF_SRC_RC as u8,
  //     rc_ctiv: 16,
  //     rc_temp_ctiv: 2,
  //     accuracy: raw::NRF_CLOCK_LF_ACCURACY_500_PPM as u8,
  //   }),
  //   conn_gap: Some(raw::ble_gap_conn_cfg_t {
  //     conn_count: 1,
  //     event_length: 24,
  //   }),
  //   conn_gatt: Some(raw::ble_gatt_conn_cfg_t { att_mtu: 256 }),
  //   gatts_attr_tab_size: Some(raw::ble_gatts_cfg_attr_tab_size_t {
  //     attr_tab_size: raw::BLE_GATTS_ATTR_TAB_SIZE_DEFAULT,
  //   }),
  //   gap_role_count: Some(raw::ble_gap_cfg_role_count_t {
  //     adv_set_count: 1,
  //     periph_role_count: 0,
  //   }),
  //   gap_device_name: Some(raw::ble_gap_cfg_device_name_t {
  //     p_value: b"HelloRust" as *const u8 as _,
  //     current_len: 9,
  //     max_len: 9,
  //     write_perm: unsafe { mem::zeroed() },
  //     _bitfield_1: raw::ble_gap_cfg_device_name_t::new_bitfield_1(0),
  //   }),
  //   ..Default::default()
  // };

  let mut config = nrf_softdevice::Config::default();
  config.gap_role_count = Some(raw::ble_gap_cfg_role_count_t {
    adv_set_count: 1,
    periph_role_count: 1,
  });
  config.conn_gap = Some(raw::ble_gap_conn_cfg_t {
    conn_count: 1,
    event_length: 24,
  });

  let softdevice = Softdevice::enable(&config);
  let server = Server::new(softdevice).unwrap();

  server.temp.temp_set(&[0x19, 0x34, 0x32, 0x00]).unwrap();

  spawner.spawn(softdevice_task(softdevice)).unwrap();

  static ADV_DATA: LegacyAdvertisementPayload =
    LegacyAdvertisementBuilder::new()
      .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
      .services_16(
        ServiceList::Complete,
        &[
          ServiceUuid16::HEALTH_THERMOMETER,
          ServiceUuid16::CURRENT_TIME,
        ],
      )
      .build();

  // but we can put it in the scan data
  // so the full name is visible once connected
  static SCAN_DATA: LegacyAdvertisementPayload =
    LegacyAdvertisementBuilder::new()
      .full_name("microbity")
      .build();

  loop {
    info!("Advertising");
    let config = peripheral::Config::default();
    let adv = peripheral::ConnectableAdvertisement::ScannableUndirected {
      adv_data: &ADV_DATA,
      scan_data: &SCAN_DATA,
    };
    let conn =
      peripheral::advertise_connectable(softdevice, adv, &config).await;

    let conn = match conn {
      Ok(conn) => conn,
      Err(e) => {
        dbg!(e);
        continue;
      }
    };

    info!("Connected");
    dbg!(&conn.conn_params());

    gatt_server::run(&conn, &server, |e| match e {
      ServerEvent::Temp(temp_e) => match temp_e {
        TempServiceEvent::TempCccdWrite { notifications } => {
          dbg!(notifications);
        }
      },
    })
    .await;
  }
}

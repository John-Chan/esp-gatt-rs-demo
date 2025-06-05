use anyhow::anyhow;
use esp_idf_svc::bt::ble::gap::{AdvConfiguration, EspBleGap};
use esp_idf_svc::bt::ble::gatt::server::EspGatts;
use esp_idf_svc::bt::{BtDriver, BtUuid};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use log::info;
use ssx10_esp::ble::gatt::BleServer;
use std::sync::Arc;

const APP_ID: u16 = 0;
const MAX_CONNECTIONS: usize = 2;

// Our service UUID
const SERVICE_UUID: u128 = 0xad91b201734740479e173bed82d75f9d;
const DEVICE_NAME: &str = "gatt-rs-demo";

pub fn run() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    let peripherals = Peripherals::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let bt = Arc::new(BtDriver::new(peripherals.modem, Some(nvs.clone()))?);

    let adv_config = AdvConfiguration {
        include_name: true,
        include_txpower: true,
        flag: 2,
        service_uuid: Some(BtUuid::uuid128(SERVICE_UUID)),
        // service_data: todo!(),
        // manufacturer_data: todo!(),
        ..Default::default()
    };
    let server = BleServer::new(
        APP_ID,
        DEVICE_NAME.to_string(),
        Arc::new(EspBleGap::new(bt.clone())?),
        Arc::new(EspGatts::new(bt.clone())?),
        adv_config,
    );
    info!("Starting gatt server,device name: {}", DEVICE_NAME);
    server.start_1().map_err(|e| anyhow!(e.to_string()))?;

    loop {
        info!("running loop");
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}
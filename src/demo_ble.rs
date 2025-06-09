use anyhow::anyhow;
use enumset::enum_set;
use esp_idf_svc::bt::ble::gap::{AdvConfiguration, AppearanceCategory, EspBleGap};
use esp_idf_svc::bt::ble::gatt::server::EspGatts;
use esp_idf_svc::bt::ble::gatt::{
    AutoResponse, GattCharacteristic, GattId, GattServiceId, Handle, Permission, Property,
};
use esp_idf_svc::bt::{BtDriver, BtUuid};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use log::info;
use ssx10_esp::ble::gatt::{
    BleServer, BtError, GattServiceHandler, GattsRef, ReadEvent, WriteEvent,
};
use std::collections::HashMap;
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
        appearance: AppearanceCategory::NetworkDevice, // or PersonalMobilityDevice?
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
    );

    server.set_ready_handler(|srv, gatt_if| {
        info!("Starting create gatt service ......");
        let adv_config = AdvConfiguration {
            include_name: true,
            include_txpower: true,
            flag: 2,
            appearance: AppearanceCategory::NetworkDevice, // or PersonalMobilityDevice?
            service_uuid: Some(BtUuid::uuid128(SERVICE_UUID)),
            // service_data: todo!(),
            // manufacturer_data: todo!(),
            ..Default::default()
        };
        srv.get_gap().set_device_name("j716-gatt")?;
        srv.get_gap().set_adv_conf(&adv_config)?;

        let svc = Arc::new(WifiCtl);
        srv.get_gatts()
            .create_service(gatt_if, &svc.service_id(), 8)?;
        Ok(vec![svc])
    });
    info!("Starting gatt server,device name: {}", DEVICE_NAME);
    server.start().map_err(|e| anyhow!(e.to_string()))?;

    loop {
        info!("running loop");
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}

pub struct WifiCtl;

impl WifiCtl {
    pub const RECV_CHARACTERISTIC_UUID: u128 = 0xb6fccb5087be44f3ae22f85485ea42c4;
    /// Our "indicate" characteristic - i.e. where clients can receive data if they subscribe to it
    pub const IND_CHARACTERISTIC_UUID: u128 = 0x503de214868246c4828fd59144da41be;

    fn add_characteristics(
        &self,
        gatts: GattsRef,
        service_id: GattServiceId,
        service_handle: Handle,
    ) -> Result<(), BtError> {
        gatts.add_characteristic(
            service_handle,
            &GattCharacteristic {
                uuid: BtUuid::uuid128(Self::RECV_CHARACTERISTIC_UUID),
                permissions: enum_set!(Permission::Write),
                properties: enum_set!(Property::Write),
                max_len: 200, // Max recv data
                auto_rsp: AutoResponse::ByApp,
            },
            &[],
        )?;

        gatts.add_characteristic(
            service_handle,
            &GattCharacteristic {
                uuid: BtUuid::uuid128(Self::IND_CHARACTERISTIC_UUID),
                permissions: enum_set!(Permission::Write | Permission::Read),
                properties: enum_set!(Property::Indicate),
                max_len: 200, // Mac iondicate data
                auto_rsp: AutoResponse::ByApp,
            },
            &[],
        )?;

        Ok(())
    }
}

impl GattServiceHandler for WifiCtl {
    fn service_id(&self) -> GattServiceId {
        let svc_uuid: u128 = 0xad91b201734740479e173bed82d72222;
        GattServiceId {
            id: GattId {
                uuid: BtUuid::uuid128(svc_uuid),
                inst_id: 0,
            },
            is_primary: true,
        }
    }

    fn on_created(
        &self,
        gatts: GattsRef,
        service_id: GattServiceId,
        handle: Handle,
    ) -> Result<(), BtError> {
        gatts
            .start_service(handle)
            .expect("Failed to start service");
        self.add_characteristics(gatts, service_id, handle)
    }

    fn on_write(&self, gatts: GattsRef, event: &WriteEvent) -> Result<(), BtError> {
        info!("On Write: WifiCtl({:?})", event);
        Ok(())
    }

    fn on_read(&self, gatts: GattsRef, event: &ReadEvent) -> Result<(), BtError> {
        info!("On Write: WifiCtl({:?})", event);
        Ok(())
    }

    fn on_connect(&self, gatts: GattsRef, conn_id: u16) -> Result<(), BtError> {
        info!("On Connect: WifiCtl,conn_id: {}", conn_id);
        Ok(())
    }

    fn on_disconnect(&self, gatts: GattsRef, conn_id: u16) -> Result<(), BtError> {
        info!("On Disconnect: WifiCtl,conn_id: {}", conn_id);
        Ok(())
    }
}
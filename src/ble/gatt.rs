use esp_idf_svc::bt::ble::gap::{AdvConfiguration, BleGapEvent, EspBleGap};
use esp_idf_svc::bt::ble::gatt::server::{ConnectionId, EspGatts, GattsEvent, TransferId};
use esp_idf_svc::bt::ble::gatt::{GattInterface, GattStatus, Handle};
use esp_idf_svc::bt::{BdAddr, Ble, BtDriver, BtStatus};
use esp_idf_svc::sys::{EspError, ESP_FAIL};
use log::{info, warn};
use std::sync::Arc;

#[derive(Debug)]
pub enum BtError {
    /// 服务数量超出限制
    ServiceLimit,
    /// 特征数量超出限制
    CharacteristicLimit,
    /// 无效的句柄
    InvalidHandle,
    /// ESP-IDF 错误
    EspError(EspError),
    /// 其他错误
    Other(&'static str),
}
pub type Result<T> = core::result::Result<T, BtError>;
pub type BleDriver = BtDriver<'static, Ble>;
pub type BleGapRef = Arc<EspBleGap<'static, Ble, Arc<BleDriver>>>;
pub type GattsRef = Arc<EspGatts<'static, Ble, Arc<BleDriver>>>;

/// GattsEvent::Write
pub struct WriteEvent {
    /// Connection id
    pub conn_id: ConnectionId,
    /// Transfer id
    pub trans_id: TransferId,
    /// The bluetooth device address which been written
    pub addr: BdAddr,
    /// The attribute handle
    pub handle: Handle,
    /// Offset of the value, if the value is too long
    pub offset: u16,
    /// The write operation need to do response
    pub need_rsp: bool,
    /// This write operation is prepare write
    pub is_prep: bool,
    /// The write attribute value
    pub value: Vec<u8>,
}

/// GattsEvent::Read
pub struct ReadEvent {
    /// Connection id
    pub conn_id: ConnectionId,
    /// Transfer id
    pub trans_id: TransferId,
    /// The bluetooth device address which been read
    pub addr: BdAddr,
    /// The attribute handle
    pub handle: Handle,
    /// Offset of the value, if the value is too long
    pub offset: u16,
    /// The value is too long or not
    pub is_long: bool,
    /// The read operation need to do response
    pub need_rsp: bool,
}
pub trait GattServiceHandler {
    fn on_write(&mut self, gatts: GattsRef, event: &WriteEvent);
    fn on_read(&mut self, gatts: GattsRef, event: &ReadEvent);
    fn on_connect(&mut self, gatts: GattsRef, conn_id: u16);
    fn on_disconnect(&mut self, gatts: GattsRef, conn_id: u16);
}

pub struct ServiceRegistry {
    services: HashMap<GattServiceId, Box<dyn GattServiceHandler>>,
}

impl ServiceRegistry {
    pub fn handle_gatts_event(&mut self, gatts: &EspGatts, event: GattsEvent) {
        match event {
            GattsEvent::Write(write_event) => {
                if let Some(service_id) = handle_map.handle_to_service.get(&write_event.attr_handle)
                {
                    if let Some(service) = self.services.get_mut(service_id) {
                        service.on_write(gatts, &write_event);
                    }
                }
            }

            GattsEvent::Read(read_event) => {
                if let Some(service_id) = handle_map.handle_to_service.get(&read_event.attr_handle)
                {
                    if let Some(service) = self.services.get_mut(service_id) {
                        service.on_read(gatts, &read_event);
                    }
                }
            }
        }
    }
}

pub fn init_services(gatts: &EspGatts) -> Result<HashMap<GattServiceId, ServiceHandles>, EspError> {
    let mut service_handles = HashMap::new();

    // 创建键盘服务
    let keyboard_service_id = GattServiceId::new(0x110A, false, 0)?;
    let keyboard_handles = gatts.create_service_and_characteristics(&keyboard_service_id, 4)?;
    service_handles.insert(keyboard_service_id, keyboard_handles);

    // 创建健康服务
    let health_service_id = GattServiceId::new(0x110C, false, 0)?;
    let health_handles = gatts.create_service_and_characteristics(&health_service_id, 3)?;
    service_handles.insert(health_service_id, health_handles);

    Ok(service_handles)
}

pub fn check_bt_status(status: BtStatus) -> Result<()> {
    if !matches!(status, BtStatus::Success) {
        warn!("Got status: {status:?}");
        Err(BtError::EspError(EspError::from_infallible::<ESP_FAIL>()))
    } else {
        Ok(())
    }
}

pub fn check_gatt_status(status: GattStatus) -> Result<()> {
    if !matches!(status, GattStatus::Ok) {
        warn!("Got status: {status:?}");
        Err(BtError::EspError(EspError::from_infallible::<ESP_FAIL>()))
    } else {
        Ok(())
    }
}
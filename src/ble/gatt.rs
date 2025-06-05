use enumset::enum_set;
use esp_idf_svc::bt::ble::gap::{AdvConfiguration, BleGapEvent, EspBleGap};
use esp_idf_svc::bt::ble::gatt::server::{ConnectionId, EspGatts, GattsEvent, TransferId};
use esp_idf_svc::bt::ble::gatt::{
    AutoResponse, GattCharacteristic, GattId, GattInterface, GattServiceId, GattStatus, Handle,
    Permission, Property,
};
use esp_idf_svc::bt::{BdAddr, Ble, BtDriver, BtStatus, BtUuid};
use esp_idf_svc::sys::{EspError, ESP_FAIL};
use log::{info, warn};
use std::collections::HashMap;
use std::fmt::Display;
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
impl Display for BtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BtError::ServiceLimit => write!(f, "Service limit reached"),
            BtError::CharacteristicLimit => write!(f, "Characteristic limit reached"),
            BtError::InvalidHandle => write!(f, "Invalid handle"),
            BtError::EspError(e) => write!(f, "EspError: {e}"),
            BtError::Other(s) => write!(f, "Other: {s}"),
        }
    }
}
impl From<EspError> for BtError {
    fn from(e: EspError) -> Self {
        BtError::EspError(e)
    }
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

pub struct ServerState {
    services: HashMap<GattServiceId, Box<dyn GattServiceHandler>>,
    attr_handler: HashMap<Handle, Vec<Handle>>,
}

pub struct GattContext {
    gap: BleGapRef,
    gatts: GattsRef,
}

#[derive(Clone)]
pub struct BleServer {
    app_id: u16,
    device_name: String,
    adv_conf: AdvConfiguration<'static>,
    gap: BleGapRef,
    gatts: GattsRef,
    //state: Arc<Mutex<ServerState>>,
}

impl BleServer {
    pub fn new(
        app_id: u16,
        device_name: String,
        gap: BleGapRef,
        gatts: GattsRef,
        adv_conf: AdvConfiguration<'static>,
    ) -> Self {
        Self {
            app_id,
            device_name,
            gap,
            gatts,
            adv_conf,
        }
    }

    //pub fn run( &server: BleServer<'a>) -> Result<()> {}
    /// Init Gap and start event
    pub fn start(&self) -> Result<()> {
        info!("BLE Gap and Gatts initialized");

        let server_cone = self.clone();
        self.gap.subscribe(move |event| {
            check_result::<(), BtError>(server_cone.handle_gap_event(event));
        })?;

        let server_cone = self.clone();
        self.gatts.subscribe(move |(gatt_if, event)| {
            check_result::<(), BtError>(server_cone.handle_gatts_event(gatt_if, event))
        })?;

        info!("BLE Gap and Gatts subscriptions initialized");

        self.gatts
            .register_app(self.app_id)
            .map_err(|e| BtError::from(e))?;

        info!("Gatts BTP app registered");

        Ok(())
    }

    fn create_service(&self, gatt_if: GattInterface) -> Result<GattServiceId> {
        //self.state.lock().unwrap().gatt_if = Some(gatt_if);

        self.gap.set_device_name(&self.device_name)?;
        self.gap.set_adv_conf(&self.adv_conf)?;

        let svc_uuid: u128 = 0xad91b201734740479e173bed82d71111;
        let service_id = GattServiceId {
            id: GattId {
                uuid: BtUuid::uuid128(svc_uuid),
                inst_id: 0,
            },
            is_primary: true,
        };
        self.gatts.create_service(gatt_if, &service_id, 8)?;

        Ok(service_id)
    }

    /// Configure and start the service
    /// Called from within the event callback once we are notified that the service is created
    fn configure_and_start_service(&self, service_handle: Handle) -> Result<()> {
        //self.state.lock().unwrap().service_handle = Some(service_handle);

        self.gatts.start_service(service_handle)?;
        self.add_characteristics(service_handle)?;

        Ok(())
    }

    pub const RECV_CHARACTERISTIC_UUID: u128 = 0xb6fccb5087be44f3ae22f85485ea42c4;
    /// Our "indicate" characteristic - i.e. where clients can receive data if they subscribe to it
    pub const IND_CHARACTERISTIC_UUID: u128 = 0x503de214868246c4828fd59144da41be;

    /// Add our two characteristics to the service
    /// Called from within the event callback once we are notified that the service is created
    fn add_characteristics(&self, service_handle: Handle) -> Result<()> {
        self.gatts.add_characteristic(
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

        self.gatts.add_characteristic(
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

    fn handle_gap_event(&self, event: BleGapEvent) -> Result<()> {
        info!("Got event: {event:?}");

        if let BleGapEvent::AdvertisingConfigured(status) = event {
            check_bt_status(status)?;
            info!("Advertising started");
            self.gap.start_advertising()?;
        }

        Ok(())
    }

    /// The main event handler for the GATTS events
    fn handle_gatts_event<'a>(&self, gatt_if: GattInterface, event: GattsEvent) -> Result<()> {
        info!("Got event: {event:?}");

        match event {
            GattsEvent::ServiceRegistered { status, app_id } => {
                info!("Service registered,status = {status:?}, app_id = {app_id}");
                check_gatt_status(status)?;
                if self.app_id == app_id {
                    self.create_service(gatt_if)?;
                }
            }
            GattsEvent::ServiceCreated {
                status,
                service_handle,
                ..
            } => {
                info!("Service created,status = {status:?}, service_handle = {service_handle}");
                check_gatt_status(status)?;
                self.configure_and_start_service(service_handle)?;
            }
            GattsEvent::CharacteristicAdded {
                status,
                attr_handle,
                service_handle,
                char_uuid,
            } => {
                info!("Characteristic added,status = {status:?}, attr_handle = {attr_handle:?}, service_handle = {service_handle:?}, char_uuid = {char_uuid:?}");
                check_gatt_status(status)?;
                //self.register_characteristic(service_handle, attr_handle, char_uuid)?;
            }
            GattsEvent::DescriptorAdded {
                status,
                attr_handle,
                service_handle,
                descr_uuid,
            } => {
                info!("Descriptor added,status = {status:?}, attr_handle = {attr_handle:?}, service_handle = {service_handle:?}, descr_uuid = {descr_uuid:?}");
                check_gatt_status(status)?;
                //self.register_cccd_descriptor(service_handle, attr_handle, descr_uuid)?;
            }
            GattsEvent::ServiceDeleted {
                status,
                service_handle,
            } => {
                info!("Service deleted,status = {status:?}, service_handle = {service_handle:?}");
                check_gatt_status(status)?;
                //self.delete_service(service_handle)?;
            }
            GattsEvent::ServiceUnregistered {
                status,
                service_handle,
                ..
            } => {
                info!(
                    "Service unregistered,status = {status:?}, service_handle = {service_handle:?}"
                );
                check_gatt_status(status)?;
                //self.unregister_service(service_handle)?;
            }
            GattsEvent::Mtu { conn_id, mtu } => {
                info!("Mtu,conn_id = {conn_id}, mtu = {mtu}");
                //self.register_conn_mtu(conn_id, mtu)?;
            }
            GattsEvent::PeerConnected { conn_id, addr, .. } => {
                info!("Peer connected,conn_id = {conn_id}, addr = {addr:?}");
                //self.create_conn(conn_id, addr)?;
            }
            GattsEvent::PeerDisconnected { addr, .. } => {
                info!("Peer disconnected,addr = {addr:?}");
                //self.delete_conn(addr)?;

                info!("Advertising restarted");
                self.gap.start_advertising()?;
            }
            GattsEvent::Write {
                conn_id,
                trans_id,
                addr,
                handle,
                offset,
                need_rsp,
                is_prep,
                value,
            } => {
                info!("Write,conn_id = {conn_id}, trans_id = {trans_id}, addr = {addr:?}, handle = {handle}, offset = {offset}, need_rsp = {need_rsp}, is_prep = {is_prep}, value = {value:?}");
                /*let handled = self.recv(
                    gatt_if, conn_id, trans_id, addr, handle, offset, need_rsp, is_prep, value,
                )?;

                if handled {
                    self.send_write_response(
                        gatt_if, conn_id, trans_id, handle, offset, need_rsp, is_prep, value,
                    )?;
                }*/
            }
            GattsEvent::Confirm { status, .. } => {
                info!("Confirm,status = {status:?}");
                check_gatt_status(status)?;
                //self.confirm_indication()?;
            }
            _ => {
                info!("Unhandled event: {event:?}");
                ()
            }
        }

        Ok(())
    }
}

/*
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
}*/

pub fn check_result<T, E: std::fmt::Debug>(status: Result<T>) {
    if let Err(e) = status {
        warn!("Got status: {e:?}");
    }
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
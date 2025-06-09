use enumset::enum_set;
use esp_idf_svc::bt::ble::gap::{AdvConfiguration, BleGapEvent, EspBleGap};
use esp_idf_svc::bt::ble::gatt::server::{ConnectionId, EspGatts, GattsEvent, TransferId};
use esp_idf_svc::bt::ble::gatt::{
    AutoResponse, GattCharacteristic, GattId, GattInterface, GattServiceId, GattStatus, Handle,
    Permission, Property,
};
use esp_idf_svc::bt::{BdAddr, Ble, BtDriver, BtStatus, BtUuid};
use esp_idf_svc::sys::{EspError, ESP_FAIL};
use log::{error, info, warn};
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::{Arc, Mutex};

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
    /// 内部错误  TODO: optional case filed
    Internal(String),
    /// 其他错误 TODO: optional case filed
    Other(String),
}
impl Display for BtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BtError::ServiceLimit => write!(f, "Service limit reached"),
            BtError::CharacteristicLimit => write!(f, "Characteristic limit reached"),
            BtError::InvalidHandle => write!(f, "Invalid handle"),
            BtError::EspError(e) => write!(f, "Esp Error: {e}"),
            BtError::Internal(s) => write!(f, "Internal Error: {s}"),
            BtError::Other(s) => write!(f, "Other Error: {s}"),
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
#[derive(Debug)]
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
#[derive(Debug)]
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
pub trait GattServiceHandler: Send + Sync {
    fn service_id(&self) -> GattServiceId;
    fn on_created(&self, gatts: GattsRef, service_id: GattServiceId, handle: Handle) -> Result<()>;
    fn on_write(&self, gatts: GattsRef, event: &WriteEvent) -> Result<()>;
    fn on_read(&self, gatts: GattsRef, event: &ReadEvent) -> Result<()>;
    fn on_connect(&self, gatts: GattsRef, conn_id: u16) -> Result<()>;
    fn on_disconnect(&self, gatts: GattsRef, conn_id: u16) -> Result<()>;
}

pub type ServiceHandleVec = Vec<Arc<dyn GattServiceHandler>>;
pub type ServiceHandleMap = HashMap<Handle, Arc<dyn GattServiceHandler>>;
pub struct ServerState {
    instances: ServiceHandleVec,
    handlers: ServiceHandleMap,
    attr_handler: HashMap<Handle, Vec<Handle>>,
}

pub type ReadyHandler =
    Box<dyn FnOnce(&BleServer, GattInterface) -> Result<ServiceHandleVec> + Send + Sync + 'static>;
#[derive(Clone)]
pub struct BleServer {
    app_id: u16,
    device_name: String,
    //adv_conf: AdvConfiguration<'static>,
    gap: BleGapRef,
    gatts: GattsRef,
    ready_handler: Arc<Mutex<Option<ReadyHandler>>>,
    state: Arc<Mutex<ServerState>>,
}

impl BleServer {
    pub fn new(
        app_id: u16,
        device_name: String,
        gap: BleGapRef,
        gatts: GattsRef,
        //adv_conf: AdvConfiguration<'static>,
    ) -> Self {
        Self {
            app_id,
            device_name,
            gap,
            gatts,
            //adv_conf,
            ready_handler: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(ServerState {
                instances: vec![],
                handlers: HashMap::new(),
                attr_handler: HashMap::new(),
            })),
        }
    }

    pub fn get_gap(&self) -> BleGapRef {
        self.gap.clone()
    }
    pub fn get_gatts(&self) -> GattsRef {
        self.gatts.clone()
    }

    pub fn set_ready_handler<F>(&self, handler: F)
    where
        F: FnOnce(&BleServer, GattInterface) -> Result<ServiceHandleVec> + Send + Sync + 'static,
    {
        self.ready_handler
            .lock()
            .expect("ready_handler lock failed")
            .replace(Box::new(handler));
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
                    //self.create_service(gatt_if)?;

                    let mut guard = match self.ready_handler.lock() {
                        Ok(g) => g,
                        Err(poison_error) => {
                            // Mutex 被污染了，通常表示前一个持有锁的线程 panic 了
                            // 在这种情况下，Mutex 内部的状态可能是不一致的。
                            eprintln!(
                                "Error: Mutex poisoned when trying to get callback: {:?}",
                                poison_error
                            );
                            return Err(BtError::Internal(format!(
                                "Error: Mutex poisoned when trying to get callback: {:?}",
                                poison_error
                            )));
                        }
                    };

                    let handler = guard.take();

                    match handler {
                        Some(func) => {
                            let services = func(&self, gatt_if)?;
                            // clear state.service and set services
                            self.state.lock().expect("failed to lock state").instances = services;
                        }
                        None => {
                            error!("Error: Ready handler not set or already executed for GattInterface: {:?}", gatt_if);
                        }
                    }
                }
            }
            GattsEvent::ServiceCreated {
                status,
                service_handle,
                service_id,
            } => {
                info!("Service created,status = {status:?}, service_handle = {service_handle}");
                check_gatt_status(status)?;
                let instance = self
                    .state
                    .lock()
                    .expect("failed to lock state")
                    .instances
                    .iter()
                    .find(|v| v.service_id().id == service_id.id)
                    .cloned();
                if let Some(instance) = instance {
                    instance.on_created(self.gatts.clone(), service_id, service_handle)?;
                    self.state
                        .lock()
                        .expect("failed to lock state")
                        .handlers
                        .insert(service_handle, instance.clone());
                } else {
                    error!("Error: Service not found for service_handle: {service_handle}");
                }
                //self.configure_and_start_service(service_handle)?;
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

struct CharHandle {
    /// Characteristic uuid
    char_uuid: BtUuid,
    /// Characteristic attribute handle
    attr_handle: Option<Handle>,
    /// Service attribute handle
    service_handle: Option<Handle>,
}

impl CharHandle {
    pub fn new(char_uuid: BtUuid) -> Self {
        CharHandle {
            char_uuid,
            attr_handle: None,
            service_handle: None,
        }
    }

    pub fn update_handle(&mut self, attr_handle: Option<Handle>, service_handle: Option<Handle>) {
        self.attr_handle = attr_handle;
        self.service_handle = service_handle;
    }
}

struct ServiceRoute {
    target: Option<Arc<dyn GattServiceHandler>>,
    service_id: GattServiceId,
    service_handle: Option<Handle>,
    char_uuids: Vec<BtUuid>,
}

#[derive(Default)]
struct RouteRegistry {
    services: Vec<ServiceRoute>,
    char_handles: Vec<CharHandle>,
}

impl RouteRegistry {
    /// Register a service,Used to declare a service
    /// params:
    /// - `service_id`: Service ID, use `service_id.id` to identify service
    /// - `char_uuids`: Declare characteristic uuid that the service contains
    /// - `target`: Service handler
    /// returns:
    /// - Error: service already registered
    pub fn register_service(
        &mut self,
        service_id: GattServiceId,
        char_uuids: &Vec<BtUuid>,
        target: Arc<dyn GattServiceHandler>,
    ) -> Result<()> {
        if let Some(_) = self
            .services
            .iter_mut()
            .find(|s| s.service_id.id == service_id.id)
        {
            Err(BtError::Internal(format!(
                "Service already registered:{:?}",
                service_id.id
            )))
        } else {
            self.services.push(ServiceRoute {
                target: Some(target),
                service_id,
                char_uuids: char_uuids.clone(),
                service_handle: None,
            });
            Ok(())
        }
    }

    /// Update service handle,Call this method when a `GattsEvent::ServiceCreated` event is received
    /// params:
    /// - `service_id`: Service ID, use `service_id.id` to identify service
    /// - `service_handle`: Service handler
    /// returns:
    /// - Error: service not register
    pub fn update_service_handle(
        &mut self,
        service_id: &GattServiceId,
        service_handle: Option<Handle>,
    ) -> Result<()> {
        if let Some(route) = self
            .services
            .iter_mut()
            .find(|s| s.service_id.id == service_id.id)
        {
            route.service_handle = service_handle;
            Ok(())
        } else {
            Err(BtError::Internal(format!(
                "Service not register:{:?}",
                service_id.id
            )))
        }
    }

    /// Update service handle,Call this method when a `GattsEvent::CharacteristicAdded` event is received
    /// params:
    /// - `char_uuid`: characteristic id
    /// - `service_handle`: Service handler
    /// - `attr_handle`: characteristic handle
    /// returns:
    /// - Error: service handle not present
    pub fn update_character_route(
        &mut self,
        char_uuid: BtUuid,
        service_handle: Handle,
        attr_handle: Handle,
    ) -> Result<()> {
        if let Some(handle) = self
            .char_handles
            .iter_mut()
            .find(|s| s.char_uuid == char_uuid)
        {
            handle.update_handle(Some(attr_handle), Some(service_handle));
            Ok(())
        } else {
            Err(BtError::Internal(format!(
                "Service handle not present:{:?}",
                service_handle
            )))
        }
    }

    pub fn dispatch_event(&mut self, event: GattsEvent) -> Result<()> {
        info!("Got event: {event:?}");

        match event {
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
                //self.gap.start_advertising()?;
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
                if let Some(route) = self.find_by_handle(service_handle, attr_handle) {}

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

    pub fn find_by_gatt_id(&self, id: GattId) -> Option<&ServiceRoute> {
        self.services.iter().find(|s| s.service_id.id == id)
    }
    pub fn find_by_service_handle(&self, handle: Handle) -> Option<&ServiceRoute> {
        self.services
            .iter()
            .find(|s| s.service_handle == Some(handle))
    }
    pub fn find_attr_handler(
        &self,
        service_handle: Handle,
        attr_handle: Handle,
    ) -> Option<Arc<dyn GattServiceHandler>> {
        if let Some(route) = self.char_handles.iter().find(|s| {
            s.service_handle == Some(service_handle) && s.attr_handle == Some(attr_handle)
        }) {
            if let Some(s) = self
                .services
                .iter()
                .find(|s| s.service_handle == Some(service_handle))
            {
                return s.target.clone();
            }
        }
        None
    }
}
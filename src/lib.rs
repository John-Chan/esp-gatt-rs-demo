pub mod bt;

pub mod bts {
    use esp_idf_svc::bt::ble::gap::{AdvConfiguration, BleGapEvent, EspBleGap};
    use esp_idf_svc::bt::ble::gatt::server::{ConnectionId, EspGatts, GattsEvent, TransferId};
    use esp_idf_svc::bt::ble::gatt::{GattInterface, GattStatus, Handle};
    use esp_idf_svc::bt::{BdAddr, Ble, BtDriver, BtStatus};
    use esp_idf_svc::sys::{EspError, ESP_FAIL};
    use log::{info, warn};
    use std::fmt::Write;
    use std::sync::{Arc, Condvar, Mutex};

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

    pub trait AttrService {
        /// 获取服务的 UUID
        fn service_uuid(&self) -> u128;

        /// 服务注册时调用
        fn on_register(&mut self, gatts: GattsRef) -> Result<()>;

        /// 客户端连接时调用
        fn on_connect(&mut self, conn_id: ConnectionId, addr: BdAddr) -> Result<()>;

        /// 客户端断开连接时调用
        fn on_disconnect(&mut self, addr: BdAddr) -> Result<()>;

        /// 处理写入请求
        fn on_write(&mut self, conn_id: ConnectionId, handle: u16, value: &[u8]) -> Result<()>;

        /// 获取服务名称
        fn name(&self) -> &str;

        /// 获取服务描述
        fn description(&self) -> &str {
            "Generic BLE Service"
        }
    }

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
    #[derive(Default)]
    struct ServerState {
        gatt_if: Option<GattInterface>,
    }
    pub struct AttrServer<'a> {
        app_id: u16,
        device_name: String,
        gap: BleGapRef,
        gatts: GattsRef,
        adv_conf: AdvConfiguration<'a>,
        state: Arc<Mutex<ServerState>>,
        cond: Arc<Condvar>,
    }

    impl AttrServer {
        pub fn new(
            app_id: u16,
            device_name: String,
            gap: BleGapRef,
            gatts: GattsRef,
            adv_conf: AdvConfiguration,
        ) -> Self {
            AttrServer {
                app_id,
                device_name,
                gap,
                gatts,
                adv_conf,
                state: Arc::new(Mutex::new(Default::default())),
                cond: Arc::new(Condvar::new()),
            }
        }

        pub fn start(services: Box<dyn AttrService>) -> Result<()> {
            info!("BLE Gap and Gatts initialized");

            let gap_server = server.clone();

            server.gap.subscribe(move |event| {
                gap_server.check_esp_status(gap_server.on_gap_event(event));
            })?;

            let gatts_server = server.clone();

            server.gatts.subscribe(move |(gatt_if, event)| {
                gatts_server.check_esp_status(gatts_server.on_gatts_event(gatt_if, event))
            })?;

            info!("BLE Gap and Gatts subscriptions initialized");

            server.gatts.register_app(APP_ID)?;

            info!("Gatts BTP app registered");

            let mut ind_data = 0_u16;

            loop {
                server.indicate(&ind_data.to_le_bytes())?;
                info!("Broadcasted indication: {ind_data}");

                ind_data = ind_data.wrapping_add(1);

                FreeRtos::delay_ms(10000);
            }
        }

        fn on_gap_event(&self, event: BleGapEvent) -> Result<()> {
            info!("Got event: {event:?}");

            if let BleGapEvent::AdvertisingConfigured(status) = event {
                self.check_bt_status(status)?;
                self.gap
                    .start_advertising()
                    .map_err(|e| BtError::EspError(e))?;
            }

            Ok(())
        }
        fn on_gatts_event(&self, gatt_if: GattInterface, event: GattsEvent) -> Result<()> {
            info!("Got event: {event:?}");

            match event {
                GattsEvent::ServiceRegistered { status, app_id } => {
                    self.check_gatt_status(status)?;
                    if self.app_id == app_id {
                        self.create_service(gatt_if)?;
                    }
                }
                GattsEvent::ServiceCreated {
                    status,
                    service_handle,
                    ..
                } => {
                    self.check_gatt_status(status)?;
                    self.configure_and_start_service(service_handle)?;
                }
                GattsEvent::CharacteristicAdded {
                    status,
                    attr_handle,
                    service_handle,
                    char_uuid,
                } => {
                    self.check_gatt_status(status)?;
                    self.register_characteristic(service_handle, attr_handle, char_uuid)?;
                }
                GattsEvent::DescriptorAdded {
                    status,
                    attr_handle,
                    service_handle,
                    descr_uuid,
                } => {
                    self.check_gatt_status(status)?;
                    self.register_cccd_descriptor(service_handle, attr_handle, descr_uuid)?;
                }
                GattsEvent::ServiceDeleted {
                    status,
                    service_handle,
                } => {
                    self.check_gatt_status(status)?;
                    self.delete_service(service_handle)?;
                }
                GattsEvent::ServiceUnregistered {
                    status,
                    service_handle,
                    ..
                } => {
                    self.check_gatt_status(status)?;
                    self.unregister_service(service_handle)?;
                }
                GattsEvent::Mtu { conn_id, mtu } => {
                    self.register_conn_mtu(conn_id, mtu)?;
                }
                GattsEvent::PeerConnected { conn_id, addr, .. } => {
                    self.create_conn(conn_id, addr)?;
                }
                GattsEvent::PeerDisconnected { addr, .. } => {
                    self.delete_conn(addr)?;
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
                    let handled = self.recv(
                        gatt_if, conn_id, trans_id, addr, handle, offset, need_rsp, is_prep, value,
                    )?;

                    if handled {
                        self.send_write_response(
                            gatt_if, conn_id, trans_id, handle, offset, need_rsp, is_prep, value,
                        )?;
                    }
                }
                GattsEvent::Confirm { status, .. } => {
                    self.check_gatt_status(status)?;
                    self.confirm_indication()?;
                }
                _ => (),
            }

            Ok(())
        }

        fn check_bt_status(&self, status: BtStatus) -> Result<()> {
            if !matches!(status, BtStatus::Success) {
                warn!("Got status: {status:?}");
                Err(BtError::EspError(EspError::from_infallible::<ESP_FAIL>()))
            } else {
                Ok(())
            }
        }

        fn check_gatt_status(&self, status: GattStatus) -> Result<()> {
            if !matches!(status, GattStatus::Ok) {
                warn!("Got status: {status:?}");
                Err(BtError::EspError(EspError::from_infallible::<ESP_FAIL>()))
            } else {
                Ok(())
            }
        }
    }
}
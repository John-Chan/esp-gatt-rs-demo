use crate::bt::service::BleService;
use crate::bt::{BleError, EspGattsRef, ServiceCommunication, ServiceEvent};
use enumset::EnumSet;
use esp_idf_svc::bt::ble::gatt::GattId;
use esp_idf_svc::{
    bt::ble::gatt::{
        server::{ConnectionId, EspGatts},
        AutoResponse, GattCharacteristic, GattServiceId, Handle, Permission, Property,
    },
    bt::{BdAddr, BtUuid},
};
use std::sync::{Arc, Mutex};

/// 数据传输服务的状态
#[derive(Default)]
struct DataTransferState {
    service_handle: Option<Handle>,
    recv_handle: Option<Handle>,
    ind_handle: Option<Handle>,
    ind_cccd_handle: Option<Handle>,
}

/// 数据传输服务
pub struct DataTransferService {
    state: Arc<Mutex<DataTransferState>>,
    gatts: Option<EspGattsRef>,
}

impl DataTransferService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DataTransferState::default())),
            gatts: None,
        }
    }

    /// 发送数据到所有已连接的客户端
    pub fn send_data(&self, data: &[u8]) -> Result<(), BleError> {
        if let Some(gatts) = &self.gatts {
            let state = self.state.lock().unwrap();
            if let Some(ind_handle) = state.ind_handle {
                // 这里应该实现实际的数据发送逻辑
                // 为简化示例，这里只打印日志
                log::info!("Sending data: {:?}", data);
            }
        }
        Ok(())
    }
}

impl ServiceCommunication for DataTransferService {
    fn notify_event(&mut self, event: ServiceEvent) {
        match event {
            ServiceEvent::MtuUpdated { mtu } => {
                log::info!("MTU updated to {}", mtu);
            }
            ServiceEvent::ConnectionChanged { connected, addr } => {
                log::info!(
                    "Connection {} for address {}",
                    if connected { "established" } else { "lost" },
                    addr
                );
            }
            ServiceEvent::Custom(msg) => {
                log::info!("Custom event received: {}", msg);
            }
        }
    }
}

impl BleService for DataTransferService {
    fn service_uuid(&self) -> u128 {
        0xad91b201734740479e173bed82d75f9d
    }

    fn on_register(&mut self, gatts: EspGattsRef) -> Result<(), BleError> {
        self.gatts = Some(gatts.clone());

        let gattid = GattId {
            uuid: BtUuid::uuid128(self.service_uuid()),
            // FIXME: inst_id
            inst_id: 0,
        };
        let service_id = GattServiceId {
            id: gattid,
            is_primary: true,
        };
        // 创建服务
        let service_handle = gatts.create_service(&service_id, 4)?;

        let mut state = self.state.lock().unwrap();
        state.service_handle = Some(service_handle);

        // 添加接收特征
        let recv_uuid = BtUuid::uuid128(0xb6fccb5087be44f3ae22f85485ea42c4);
        let recv_char = GattCharacteristic {
            uuid: recv_uuid,
            permissions: EnumSet::from(Permission::Write),
            properties: EnumSet::from(Property::Write),
            max_len: 512,
            auto_rsp: AutoResponse::ByApp,
        };
        let recv_handle = gatts.add_characteristic(service_handle, &recv_char)?;
        state.recv_handle = Some(recv_handle);

        // 添加指示特征
        let ind_uuid = BtUuid::uuid128(0x503de214868246c4828fd59144da41be);
        let ind_char = GattCharacteristic {
            uuid: ind_uuid,
            permissions: Permission::READ | Permission::WRITE,
            properties: Property::INDICATE,
            max_len: 512,
            auto_rsp: AutoResponse::ByApp,
        };
        let ind_handle = gatts.add_characteristic(service_handle, &ind_char)?;
        state.ind_handle = Some(ind_handle);

        // 启动服务
        gatts.start_service(service_handle)?;

        Ok(())
    }

    fn on_connect(&mut self, conn_id: ConnectionId, addr: BdAddr) -> Result<(), BleError> {
        log::info!("Client connected: {} (conn_id: {})", addr, conn_id);
        Ok(())
    }

    fn on_disconnect(&mut self, addr: BdAddr) -> Result<(), BleError> {
        log::info!("Client disconnected: {}", addr);
        Ok(())
    }

    fn on_write(
        &mut self,
        conn_id: ConnectionId,
        handle: u16,
        value: &[u8],
    ) -> Result<(), BleError> {
        let state = self.state.lock().unwrap();
        if Some(handle) == state.recv_handle {
            log::info!(
                "Received data from conn_id {}: {:?}",
                conn_id,
                String::from_utf8_lossy(value)
            );
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "Data Transfer Service"
    }

    fn description(&self) -> &str {
        "A service for bidirectional data transfer"
    }
}
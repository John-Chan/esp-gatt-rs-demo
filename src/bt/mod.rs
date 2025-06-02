use std::sync::{Arc};

use esp_idf_svc::{
    bt::ble::{
        gap::{AdvConfiguration, BleGapEvent, EspBleGap},
        gatt::{
            server::{ConnectionId, EspGatts, GattsEvent},
            GattInterface, Handle,
        },
    },
    sys::EspError,
};
use esp_idf_svc::bt::{Ble, BtDriver};

pub mod gatt;
pub mod service;

use service::BleService;

/// BLE 服务器错误类型
#[derive(Debug)]
pub enum BleError {
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

impl From<EspError> for BleError {
    fn from(error: EspError) -> Self {
        BleError::EspError(error)
    }
}

/// 服务间通信的事件类型
#[derive(Debug, Clone)]
pub enum ServiceEvent {
    /// 连接状态改变
    ConnectionChanged { connected: bool, addr: String },
    /// MTU 更新
    MtuUpdated { mtu: u16 },
    /// 自定义事件
    Custom(String),
}

/// 服务间通信的 trait
pub trait ServiceCommunication {
    /// 处理来自其他服务的事件
    fn notify_event(&mut self, event: ServiceEvent);
}

pub type BleDriver = BtDriver<'static, Ble>;
pub type EspBleGapRef = Arc<EspBleGap<'static, Ble, Arc<BleDriver>>>;
pub type EspGattsRef = Arc<EspGatts<'static, Ble, Arc<BleDriver>>>;

/// BLE 服务器管理器
pub struct BleServer {
    gap: EspBleGapRef,
    gatts: EspGattsRef,
    services: Vec<Box<dyn BleService>>,
}

impl BleServer {
    /// 创建新的 BLE 服务器实例
    pub fn new(gap: EspBleGapRef, gatts: EspGattsRef) -> Self {
        Self {
            gap,
            gatts,
            services: Vec::new(),
        }
    }

    /// 添加新的服务
    pub fn add_service<S: BleService + 'static>(&mut self, service: S) -> Result<(), BleError> {
        // 检查服务数量限制
        if self.services.len() >= 5 {
            // 假设最多支持5个服务
            return Err(BleError::ServiceLimit);
        }
        self.services.push(Box::new(service));
        Ok(())
    }

    /// 启动 BLE 服务器
    pub fn start(&mut self) -> Result<(), BleError> {
        // 注册所有服务
        for service in &mut self.services {
            self.register_service(service)?;
        }

        // 配置广播
        self.configure_advertising()?;

        // 开始广播
        self.start_advertising()?;

        Ok(())
    }

    /// 注册服务
    fn register_service(&mut self, service: &mut Box<dyn BleService>) -> Result<(), BleError> {
        service.on_register(self.gatts.clone())?;
        Ok(())
    }

    /// 配置广播
    fn configure_advertising(&self) -> Result<(), BleError> {
        let config = AdvConfiguration {
            include_name: true,
            include_txpower: true,
            flag: 2,
            ..Default::default()
        };
        self.gap.set_device_name("ESP32-BLE")?;
        self.gap.set_adv_conf(&config)?;
        Ok(())
    }

    /// 开始广播
    fn start_advertising(&self) -> Result<(), BleError> {
        self.gap.start_advertising()?;
        Ok(())
    }

    /// 处理 GATTS 事件
    pub fn on_gatts_event(
        &mut self,
        gatt_if: GattInterface,
        event: GattsEvent,
    ) -> Result<(), BleError> {
        match event {
            GattsEvent::Write {
                conn_id,
                handle,
                value,
                ..
            } => {
                // 分发写入事件到相应的服务
                for service in &mut self.services {
                    if let Err(e) = service.on_write(conn_id, handle, value) {
                        log::warn!("Service write handler error: {:?}", e);
                    }
                }
            }
            GattsEvent::PeerConnected { conn_id, addr, .. } => {
                // 通知所有服务有新连接
                for service in &mut self.services {
                    if let Err(e) = service.on_connect(conn_id, addr) {
                        log::warn!("Service connect handler error: {:?}", e);
                    }
                }
            }
            GattsEvent::PeerDisconnected { addr, .. } => {
                // 通知所有服务连接断开
                for service in &mut self.services {
                    if let Err(e) = service.on_disconnect(addr) {
                        log::warn!("Service disconnect handler error: {:?}", e);
                    }
                }
            }
            GattsEvent::Mtu { conn_id, mtu } => {
                // 通知所有服务 MTU 更新
                let event = ServiceEvent::MtuUpdated { mtu };
                for service in &mut self.services {
                    service.notify_event(event.clone());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 处理 GAP 事件
    pub fn on_gap_event(&mut self, event: BleGapEvent) -> Result<(), BleError> {
        match event {
            BleGapEvent::AdvertisingConfigured(status) => {
                if status.is_ok() {
                    self.start_advertising()?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

use esp_idf_svc::{
    bt::ble::gatt::server::{ConnectionId, EspGatts},
    bt::BdAddr,
    sys::EspError,
};
use std::sync::Arc;

use super::{BleError, ServiceCommunication};

/// BLE 服务的基本特征
pub trait BleService: ServiceCommunication {
    /// 获取服务的 UUID
    fn service_uuid(&self) -> u128;

    /// 服务注册时调用
    fn on_register(&mut self, gatts: Arc<EspGatts<'static>>) -> Result<(), BleError>;

    /// 客户端连接时调用
    fn on_connect(&mut self, conn_id: ConnectionId, addr: BdAddr) -> Result<(), BleError>;

    /// 客户端断开连接时调用
    fn on_disconnect(&mut self, addr: BdAddr) -> Result<(), BleError>;

    /// 处理写入请求
    fn on_write(
        &mut self,
        conn_id: ConnectionId,
        handle: u16,
        value: &[u8],
    ) -> Result<(), BleError>;

    /// 获取服务名称
    fn name(&self) -> &str;

    /// 获取服务描述
    fn description(&self) -> &str {
        "Generic BLE Service"
    }
}

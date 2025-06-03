use esp_idf_svc::ble::*;
use esp_idf_svc::hal::prelude::*;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use std::collections::HashMap;

// 前面定义的 ServiceRegistry 和 BleService trait
trait BleService {
    fn service_id(&self) -> &GattServiceId;
    fn create(&mut self, gatts: &EspGatts) -> Result<ServiceHandles, EspError>;
    fn handle_event(&mut self, gatts: &EspGatts, event: &GattsEvent);
    fn delete(&self, gatts: &EspGatts);
}

struct ServiceHandles {
    service_handle: u16,
    char_handles: Vec<u16>,
}

struct ServiceRegistry {
    services: HashMap<GattServiceId, Box<dyn BleService>>,
    handles_map: HashMap<u16, GattServiceId>,
}

impl ServiceRegistry {
    fn new() -> Self {
        ServiceRegistry {
            services: HashMap::new(),
            handles_map: HashMap::new(),
        }
    }

    fn register(&mut self, service: Box<dyn BleService>) {
        self.services.insert(service.service_id().clone(), service);
    }

    fn create_all_services(&mut self, gatts: &EspGatts) -> Result<(), EspError> {
        for (id, service) in self.services.iter_mut() {
            let handles = service.create(gatts)?;
            for handle in &handles.char_handles {
                self.handles_map.insert(*handle, id.clone());
            }
        }
        Ok(())
    }

    fn route_event(&mut self, gatts: &EspGatts, event: GattsEvent) {
        match event {
            GattsEvent::Write(write_event) => {
                if let Some(service_id) = self.handles_map.get(&write_event.attr_handle) {
                    if let Some(service) = self.services.get_mut(service_id) {
                        service.handle_event(gatts, &event);
                    }
                }
            }
            GattsEvent::Read(read_event) => {
                if let Some(service_id) = self.handles_map.get(&read_event.attr_handle) {
                    if let Some(service) = self.services.get_mut(service_id) {
                        service.handle_event(gatts, &event);
                    }
                }
            }
            _ => {}
        }
    }
}

// 示例服务：键盘服务
struct KeyboardService {
    handles: Option<ServiceHandles>,
}

impl BleService for KeyboardService {
    fn service_id(&self) -> &GattServiceId {
        static SERVICE_ID: GattServiceId = GattServiceId::new(0x110A, false, 0);
        &SERVICE_ID
    }

    fn create(&mut self, gatts: &EspGatts) -> Result<ServiceHandles, EspError> {
        let service_handle = gatts.create_service(GattInterface::Primary, self.service_id(), 4)?;
        let char_config = GattCharacteristicConfig {
            uuid: Uuid::Uuid16(0x2A50),
            props: GattCharacteristicProp::READ | GattCharacteristicProp::WRITE,
            permissions: GattPermission::READABLE | GattPermission::WRITABLE,
            value_len: 20,
            ..Default::default()
        };
        let char_handle = gatts.add_characteristic(service_handle, char_config)?;
        self.handles = Some(ServiceHandles {
            service_handle,
            char_handles: vec![char_handle],
        });
        gatts.start_service(service_handle)?;
        Ok(self.handles.as_ref().unwrap().clone())
    }

    fn handle_event(&mut self, gatts: &EspGatts, event: &GattsEvent) {
        if let GattsEvent::Write(write_event) = event {
            log::info!("Keyboard service received write: {:?}", write_event.value);
            if write_event.need_rsp {
                gatts
                    .send_response(write_event.conn_id, write_event.trans_id, Ok(()))
                    .expect("Failed to send response");
            }
        }
    }

    fn delete(&self, gatts: &EspGatts) {
        if let Some(ref handles) = self.handles {
            gatts.delete_service(handles.service_handle).ok();
        }
    }
}

// GapServer 结构体
pub struct GapServer<'a> {
    ble_gap: &'a EspBleGap,
}

impl<'a> GapServer<'a> {
    pub fn new(ble_gap: &'a EspBleGap) -> Self {
        GapServer { ble_gap }
    }

    pub fn init(&self) -> anyhow::Result<()> {
        // 设置设备名称
        self.ble_gap.set_device_name("ESP_BLE_DEVICE")?;
        Ok(())
    }

    pub fn start_advertising(&self) -> anyhow::Result<()> {
        let adv_data = ble_gap::AdvertisingDataBuilder::new()
            .name(Some("ESP_BLE_DEVICE"))
            .flags(ble_gap::BLE_HS_ADV_F_DISC_GEN | ble_gap::BLE_HS_ADV_F_BREDR_NOT_SUPPORTED)
            .build()?;
        self.ble_gap.start_advertising(adv_data)?;
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    let peripherals = Peripherals::take().unwrap();
    let sysloop = EspEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // 初始化 BLE 控制器
    let ble_controller = EspBleController::new(peripherals.ble, sysloop.clone(), nvs)?;
    ble_controller.init()?;
    let ble_gap = ble_controller.gap();
    let ble_gatts = ble_controller.gatts();

    // 初始化 GapServer
    let gap_server = GapServer::new(ble_gap);
    gap_server.init()?;
    gap_server.start_advertising()?;

    // 初始化服务注册表并添加服务
    let mut registry = ServiceRegistry::new();
    registry.register(Box::new(KeyboardService { handles: None }));
    // 可继续添加其他服务...

    registry.create_all_services(&ble_gatts)?;

    // 主循环处理 GATT 事件
    loop {
        while let Ok(event) = ble_gatts.next_event()? {
            registry.route_event(&ble_gatts, event);
        }
    }
}
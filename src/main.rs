//! https://github.com/esp-rs/esp-idf-svc/blob/master/examples/bt_gatt_server.rs
//! Example of a BLE GATT server using the ESP IDF Bluedroid BLE bindings.
//! Build with `--features experimental` (for now).
//!
//! You can test it with any "GATT Browser" app, like e.g.
//! the "GATTBrowser" mobile app available on Android.
//!
//! The example server publishes a single service featuring two characteristics:
//! - A "recv" characteristic that clients can write to
//! - An "indicate" characteristic that clients can subscribe to and receive indications from
//!
//! The example is relatively sophisticated as it demonstrates not only how to receive data from clients
//! but also how to broadcast data to all clients that have subscribed to a characteristic, including
//! handling indication confirmations.
//!
//! Note that the Buedroid stack consumes a lot of memory, so `sdkconfig.defaults` should be carefully configured
//! to avoid running out of memory.
//!
//! Here's a working configuration, but you might need to adjust further to your concrete use-case:
//!
//! CONFIG_BT_ENABLED=y
//! CONFIG_BT_BLUEDROID_ENABLED=y
//! CONFIG_BT_CLASSIC_ENABLED=n
//! CONFIG_BTDM_CTRL_MODE_BLE_ONLY=y
//! CONFIG_BTDM_CTRL_MODE_BR_EDR_ONLY=n
//! CONFIG_BTDM_CTRL_MODE_BTDM=n
//! CONFIG_BT_BLE_42_FEATURES_SUPPORTED=y
//! CONFIG_BT_BLE_50_FEATURES_SUPPORTED=n
//! CONFIG_BT_BTC_TASK_STACK_SIZE=15000
//! CONFIG_BT_BLE_DYNAMIC_ENV_MEMORY=y

#![allow(unknown_lints)]
#![allow(unexpected_cfgs)]

mod demo_ble;
mod example;
#[cfg(all(not(esp32s2), feature = "experimental"))]
fn main() -> anyhow::Result<()> {
    demo_ble::run()
}

#[cfg(any(esp32s2, not(feature = "experimental")))]
fn main() -> anyhow::Result<()> {
    #[cfg(esp32s2)]
    panic!("ESP32-S2 does not have a BLE radio");

    #[cfg(not(feature = "experimental"))]
    panic!("Use `--features experimental` when building this example");
}
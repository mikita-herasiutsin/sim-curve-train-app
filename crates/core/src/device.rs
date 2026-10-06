//! Game controller descriptions shared between the input backend and the UI.

use serde::Serialize;

/// A connected game controller as reported by the input backend (SDL3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Backend instance id. Only valid while the device stays connected.
    pub id: u32,
    pub name: String,
    /// Stable device GUID as 32 hex characters; device profiles are keyed by it.
    pub guid: String,
    /// USB vendor id, if the GUID carries one.
    pub vendor_id: Option<u16>,
    /// USB product id, if the GUID carries one.
    pub product_id: Option<u16>,
    pub axis_count: u32,
    pub button_count: u32,
    pub hat_count: u32,
}

/// The device list as last seen by the input backend.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicesSnapshot {
    /// Connected devices, sorted by name and then id.
    pub devices: Vec<DeviceInfo>,
    /// Set when the input backend failed to start; `devices` is empty then.
    pub error: Option<String>,
}

/// Extracts the USB vendor and product ids from an SDL joystick GUID string.
///
/// SDL GUIDs are 8 little-endian `u16` words: bus, CRC, vendor, 0, product, 0, version, driver.
/// Returns `None` for malformed strings and for GUIDs that don't follow that layout
/// (e.g. devices identified only by name).
#[must_use]
pub fn usb_ids_from_guid(guid: &str) -> Option<(u16, u16)> {
    if guid.len() != 32 || !guid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let word = |i: usize| -> Option<u16> {
        let lo = u8::from_str_radix(&guid[i * 4..i * 4 + 2], 16).ok()?;
        let hi = u8::from_str_radix(&guid[i * 4 + 2..i * 4 + 4], 16).ok()?;
        Some(u16::from_le_bytes([lo, hi]))
    };
    let (vendor, product) = (word(2)?, word(4)?);
    if word(3)? != 0 || word(5)? != 0 || vendor == 0 {
        return None;
    }
    Some((vendor, product))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_usb_guid() {
        // Xbox 360 controller over USB (bus 0x0003, VID 045e, PID 028e).
        assert_eq!(
            usb_ids_from_guid("030000005e0400008e02000000007200"),
            Some((0x045e, 0x028e))
        );
    }

    #[test]
    fn rejects_guid_without_usb_layout() {
        // Name-hashed GUID: non-zero padding words.
        assert_eq!(usb_ids_from_guid("05000000536f6e7920436f6d70757400"), None);
        // Zero vendor.
        assert_eq!(usb_ids_from_guid("03000000000000008e02000000000000"), None);
    }

    #[test]
    fn rejects_malformed_guid() {
        assert_eq!(usb_ids_from_guid(""), None);
        assert_eq!(usb_ids_from_guid("0300"), None);
        assert_eq!(usb_ids_from_guid("zz0000005e0400008e02000000007200"), None);
        assert_eq!(usb_ids_from_guid("030000005e0400008e0200000000720é"), None);
    }

    #[test]
    fn snapshot_serializes_camel_case() {
        let snapshot = DevicesSnapshot {
            devices: vec![DeviceInfo {
                id: 7,
                name: "Pedals".into(),
                guid: "030000005e0400008e02000000007200".into(),
                vendor_id: Some(0x045e),
                product_id: Some(0x028e),
                axis_count: 3,
                button_count: 0,
                hat_count: 0,
            }],
            error: None,
        };
        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(json["devices"][0]["axisCount"], 3);
        assert_eq!(json["devices"][0]["vendorId"], 0x045e);
        assert_eq!(json["error"], serde_json::Value::Null);
    }
}

// This file contains utilities

// bluetooth address is 6 bytes. put ':' character as a seperator.
pub fn format_bluetooth_address(value: u64) -> String {
    let bytes = [
        // Bluetooth device address is 6 bytes. Omit first 2 bytes of u64.
        // (value >> 56) as u8,
        // (value >> 48) as u8,
        (value >> 40) as u8,
        (value >> 32) as u8,
        (value >> 24) as u8,
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ];

    return format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5]);
}

// parse a given string as u64. it can take following forms:
//  * "112233445566" - no delimotor
//  * "11:22:33:44:55:66" - : delimited
pub fn decode_bluetooth_adddress(value: &str) -> Result<u64, &'static str> {
    // In case value is HEX integer without a delimiter
    if let Ok(decoded) = u64::from_str_radix(&value, 16) {
        return Ok(decoded);
    }

    // Let's split string into 6 pieces.
    let bytes: Vec<&str> = value.split(':').collect();
    if bytes.len() != 6 {
        return Err("given address is not 6 byte form");
    }

    let mut converted: u64 = 0;

    for b in bytes {
        match u8::from_str_radix(b, 16) {
            Ok(v) => {
                converted = (converted << 8) | (v as u64);
            },
            Err(_) => {
                return Err("parsing error.");
            }
        }
    }

    return Ok(converted);
}

// Returning unix epoch time. Timezone is UTC.
pub fn get_unix_epoc() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now();
    let duration = now.duration_since(UNIX_EPOCH).expect("failed to get UNIX_EPOCH");
    return duration.as_secs();
}

use chrono::Offset;
use serde::{Deserialize, Deserializer, de::Error};

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "device")]
    pub devices: Option<Vec<DeviceConfig>>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceConfig {
    // Bluetooth device's address
    #[serde(deserialize_with = "string_to_bluetooth_address")]
    pub address: u64,
    // Name of device
    pub name: Option<String>,
    // Omit this device. We will not sync.
    pub omit: Option<bool>,
    // Timezone declared by https://docs.rs/chrono-tz/latest/chrono_tz/
    pub timezone: Option<String>,
    pub offset_seconds: Option<i32>,
}

// Custom parser for bluetooth address string.
fn string_to_bluetooth_address<'de, D>(deserializer: D) -> Result<u64, D::Error> 
where
    D: Deserializer<'de>
{
    let s = String::deserialize(deserializer)?;
    match decode_bluetooth_adddress(&s) {
        Ok(decoded) => Ok(decoded),
        Err(msg) => Err(D::Error::custom(msg)),
    }
}

#[allow(dead_code)]
impl Config {
    pub fn get_device_by_name(&self, name: &str) -> Option<&DeviceConfig> {
        for d in self.devices.iter().flatten() {
            if let Some(n) = &d.name {
                if n.to_lowercase() == name.to_lowercase() {
                    return Some(d);
                }
            }
        }
        return None;
    }
}

impl DeviceConfig {
    pub fn get_timezone_diff_hour(&self) -> Option<i8> {
        use chrono::{Utc, DateTime};
        use chrono_tz::Tz;

        if let Some(name) = &self.timezone {
            let tz: Tz = name.parse().unwrap();

            // Get utc time.
            let utc_now: DateTime<Utc> = Utc::now();
            let local_now: DateTime<Tz> = utc_now.with_timezone(&tz);
            let diff_seconds = local_now.offset().fix().local_minus_utc();

            return Some((diff_seconds / 3600) as i8);
        }

        return None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bluetooth_address() {
        assert_eq!(format_bluetooth_address(0x112233445566), "11:22:33:44:55:66");

        // Use upper case.
        assert_eq!(format_bluetooth_address(0x0a0b0c0d0e0f), "0A:0B:0C:0D:0E:0F");
    }

    #[test]
    fn test_decode_bluetooth_adddress() {
        // normal hex form.
        assert_eq!(decode_bluetooth_adddress("112233445566").unwrap(), 0x112233445566);

        // delimited
        assert_eq!(decode_bluetooth_adddress("11:22:33:44:55:66").unwrap(), 0x112233445566);
        assert_eq!(decode_bluetooth_adddress("A:b:c:d:e:f").unwrap(), 0x0a0b0c0d0e0f);

        // error checking.
        assert!(decode_bluetooth_adddress("11:22").is_err());
        assert!(decode_bluetooth_adddress("11:22:33:44:55:66:77").is_err());
    }

    #[test]
    fn test_get_unix_epoc() {
        assert!(get_unix_epoc() != 0);
    }

    #[test]
    fn test_toml1() {
        let s = r#"
        [[device]]
        address = "11:22:33:44:55:66"
        name = "test1"
        "#;

        let config: Config = toml::from_str(&s).unwrap();
        assert!(config.devices.is_some());
    }

    #[test]
    fn test_toml() {
        let s = r#"
        [[device]]
        address = "11:22:33:44:55:66"
        name = "test1"
        omit = true
        offset_seconds = +32
        timezone = "US/Pacific"

        [[device]]
        address = "665544332211"
        name = "test2"
        timezone = "Asia/Seoul"
        "#;

        let config: Config = toml::from_str(&s).unwrap();
        assert!(config.devices.is_some());

        {
            let test1 = config.get_device_by_name("test1").unwrap();
            assert_eq!(test1.address, 0x112233445566);
            assert_eq!(test1.name.as_ref().unwrap(), "test1");
            assert_eq!(test1.omit.unwrap(), true);
            assert_eq!(test1.offset_seconds.unwrap(), 32);
            assert_eq!(test1.timezone.as_ref().unwrap(), "US/Pacific");

            let diff = test1.get_timezone_diff_hour().unwrap();
            assert!(diff == -7 || diff == -8);
        }

        {
            let test2 = config.get_device_by_name("test2").unwrap();
            assert_eq!(test2.address, 0x665544332211);
            assert_eq!(test2.name.as_ref().unwrap(), "test2");
            assert!(test2.omit.is_none());
            assert!(test2.offset_seconds.is_none());

            let diff = test2.get_timezone_diff_hour().unwrap();
            assert_eq!(diff, 9);
        }
    }

}
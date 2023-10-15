use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::Sender,
    sync::{Arc, Mutex},
};

use windows::{
    core::GUID,
    Devices::Bluetooth::{
        Advertisement::{*},
        BluetoothLEDevice,
        GenericAttributeProfile::{
            GattDeviceService,
            GattCommunicationStatus, GattCharacteristic},
    },
    // Foundation::TypedEventHandler
};

use xiaomi::get_unix_epoc;
use xiaomi::DeviceConfig;

// this is not xiaomi specific, it could be reported from any other BLE devices.
const ENVIRONMENTAL_SENSING_SERVICE_UUID: GUID = GUID::from_u128(0x0000181a00001000800000805f9b34fb);   // "0000181a-0000-1000-8000-00805f9b34fb"
const LYWSD02_SERVICE_UUID: GUID = GUID::from_u128(0xEBE0CCB07A0A4B0C8A1A6FF2997DA3A6); // "EBE0CCB0-7A0A-4B0C-8A1A-6FF2997DA3A6"
const LYWSD02_CHARACTERISTIC_TIME_UUID: GUID = GUID::from_u128(0xEBE0CCB77A0A4B0C8A1A6FF2997DA3A6); // "EBE0CCB7-7A0A-4B0C-8A1A-6FF2997DA3A6"

pub struct SensorValue {
    pub address: u64,
    pub value: f32,
}

pub enum AdvertisementKind {
    // For now all other advertisements are unknown.
    Unknown,
    // Omit is from xiaomi device. Having 'address' field might be better in future.
    Omit,
    // Following 3 are data sent from xiaomi device.
    Temperature(SensorValue),
    Humidity(SensorValue),
    Battery(SensorValue),
}

// decode advertisement packet. especially, decode the xiaomi's temperature / humidity packet.
pub fn decode_advertisement(args: &Option<BluetoothLEAdvertisementReceivedEventArgs>) -> AdvertisementKind {
    if let Some(args) = args {
        let advertisement = args.Advertisement().unwrap();
        let services = advertisement.ServiceUuids().unwrap();
        let has_services = services.Size().unwrap() != 0;
        let has_xiaomi_service = has_services && services.into_iter().find(|&x| x == ENVIRONMENTAL_SENSING_SERVICE_UUID) == Some(ENVIRONMENTAL_SENSING_SERVICE_UUID);

        if has_xiaomi_service {
            //let address_type = args.BluetoothAddressType().unwrap();
            let address64 = args.BluetoothAddress().unwrap();

            for section in advertisement.DataSections().unwrap() {
                let data_type = section.DataType().unwrap();

                // ServiceData
                if data_type == 0x16 {
                    let data = section.Data().unwrap();
                    let reader = windows::Storage::Streams::DataReader::FromBuffer(&data).unwrap();
                    let mut vector: Vec<u8> = Vec::new();
                    vector.resize(data.Length().unwrap() as usize, 0);
                    reader.ReadBytes(vector.as_mut_slice()).ok();

                    // Temperature and Humidity are using 2 bytes. Combine them and convert into f32.
                    // Battery is percentage, just single byte.
                    match vector[14] {
                        4 => { // temperature
                            let v1 = (vector[17] as i32) + (vector[18] as i32) * 256;
                            let v2 = (v1 as f32) / 10.0;
                            return AdvertisementKind::Temperature(SensorValue{ address: address64, value: v2 });
                        },
                        6 => { // humidity
                            let v1 = (vector[17] as i32) + (vector[18] as i32) * 256;
                            let v2 = (v1 as f32) / 10.0;
                            return AdvertisementKind::Humidity(SensorValue{ address: address64, value: v2 });
                        },
                        10 => { // battery
                            let v = vector[17] as f32;
                            return AdvertisementKind::Battery(SensorValue{ address: address64, value: v });
                        },
                        _ => {
                            return AdvertisementKind::Unknown;
                        }
                    }
                }
            }

            // This has xiaomi service data, but we don't know the format. Let's omit.
            return AdvertisementKind::Omit;
        }
    }

    // Advertisement from Unknown device.
    return AdvertisementKind::Unknown;
}

pub enum SyncLogKind {
    Progress{ address: u64, log: String },
    Error{ address: u64, log: String },
}

pub fn sync_device_args(config: &Arc<Mutex<HashMap<u64, DeviceConfig>>>, handled_devices: &Arc<Mutex<HashSet<u64>>>, sender: &Sender<SyncLogKind>, args: &Option<BluetoothLEAdvertisementReceivedEventArgs>) {
    // decode advertisement and return the address if it is xiaomi temperature sensor.
    // otherwise, we will omit this advertisement.
    let get_address = |args: &Option<BluetoothLEAdvertisementReceivedEventArgs>| -> Option<u64> {
        match decode_advertisement(&args) {
            AdvertisementKind::Temperature(v) | 
            AdvertisementKind::Humidity(v) => Some(v.address),

            // Battery might be sent from other devices. So omit this.
            _ => None,
        }
    };

    // return true if this device is already handled successfully.
    let is_handled = |address: u64| -> bool {
        let handled_devices = handled_devices.lock().unwrap();
        return handled_devices.contains(&address);
    };

    // see if this device is omitable.
    let is_omit = |address: u64| -> bool {
        if let Some(device) = config.lock().unwrap().get(&address) {
            if let Some(omit) = device.omit {
                return omit;
            }
        }
        return false;
    };

    // advertisement looks xiaomi temperature sensor,
    // and we didn't handle the device before.
    if let Some(address) = get_address(&args) {
        if is_handled(address) {
            // do nothing
        }
        else if is_omit(address) {
            // mark this device is handled.
            let mut handled_devices = handled_devices.lock().unwrap();
            handled_devices.insert(address);

            sender.send(SyncLogKind::Progress { address: address, log: "Configured as Omit".to_string() }).unwrap();
        }
        else {
            let mut timezone_hour: Option<i8> = None;
            let mut offset_seconds: Option<i32> = None;

            if let Some(device_config) = config.lock().unwrap().get(&address) {
                timezone_hour = device_config.get_timezone_diff_hour();
                offset_seconds = device_config.offset_seconds;
            }

            match sync_xiaomi_clock(sender, address, timezone_hour, offset_seconds) {
                Ok(_) => {
                    let mut handled_devices = handled_devices.lock().unwrap();
                    handled_devices.insert(address);
                },
                Err(msg) => {
                    sender.send(SyncLogKind::Error { address: address, log: msg }).unwrap();
                }
            }
        }
    }
}

fn log_sync_progress(sender: &Sender<SyncLogKind>, address: u64, msg: &str) {
    sender.send(SyncLogKind::Progress { address: address, log: msg.to_string() }).unwrap();
}

fn sync_xiaomi_clock(sender: &Sender<SyncLogKind>, address: u64, timezone_diff_hour: Option<i8>, offset_seconds: Option<i32>) -> Result<(), String> {
    log_sync_progress(sender, address, "Connecting...");
    let device: Option<BluetoothLEDevice>;
    match BluetoothLEDevice::FromBluetoothAddressAsync(address).unwrap().get() {
        Err(_) => { return Err("Failed to connect".to_string()); }
        Ok(d) => {
            device = Some(d);
        }
    }

    log_sync_progress(sender, address, &format!("Querying service, UUID={:x}", LYWSD02_SERVICE_UUID.to_u128()));
    let service: Option<GattDeviceService>;
    match device.unwrap().GetGattServicesForUuidAsync(LYWSD02_SERVICE_UUID).unwrap().get() {
        Err(_) => { return Err("Failed to query service".to_string()); }
        Ok(ss) => {
            if ss.Status().unwrap() != GattCommunicationStatus::Success {
                return Err("Communication error".to_string());
            }

            let services = ss.Services().unwrap();
            if services.Size().unwrap() == 0 {
                return Err("No services returned".to_string());
            }

            service = Some(services.GetAt(0).unwrap());
        }
    }

    log_sync_progress(sender, address, &format!("Querying characteristic, UUID={:x}", LYWSD02_CHARACTERISTIC_TIME_UUID.to_u128()));
    let character: Option<GattCharacteristic>;
    match service.unwrap().GetCharacteristicsForUuidAsync(LYWSD02_CHARACTERISTIC_TIME_UUID).unwrap().get() {
        Err(_) => { return Err("Failed to query characteristic".to_string()); }
        Ok(res) => {
            if res.Status().unwrap() != GattCommunicationStatus::Success {
                return Err("Communication error".to_string());
            }

            let chars = res.Characteristics().unwrap();
            if chars.Size().unwrap() == 0 {
                return Err("No characteristic returned".to_string());
            }

            character = Some(chars.GetAt(0).unwrap());
        }
    }

    let mut epoch_time: u64 = get_unix_epoc();
    let mut timezone: i8 = 9;   // Default to Korean standard time

    if let Some(tz) = timezone_diff_hour {
        if tz >= -24 && tz <= 24 {
            timezone = tz;
        }
    }

    // Adjust offset
    if let Some(diff) = offset_seconds {
        let mut temp = epoch_time as i64;
        temp = temp + (diff as i64);
        if temp > 0 {
            epoch_time = temp as u64;
            log_sync_progress(sender, address, &format!("Adjust clock {:+}:{:02} ", diff / 60, diff % 60));
        }
    }

    // Create a buffer to sync
    use windows::Storage::Streams::{DataWriter, IBuffer, ByteOrder};
    let buffer: Option<IBuffer>;
    {
        let data_writer = DataWriter::new().unwrap();
        data_writer.SetByteOrder(ByteOrder::LittleEndian).ok();
        data_writer.WriteUInt32(epoch_time as u32).ok();
        data_writer.WriteByte(timezone as u8).ok();
        buffer = Some(data_writer.DetachBuffer().unwrap());
    }
    
    // Send time to device.
    match character.unwrap().WriteValueAsync(&buffer.unwrap()).unwrap().get() {
        Err(_) => { return Err("Failed to sync time".to_string()); },
        Ok(_) => {}
    }

    log_sync_progress(sender, address, &format!("Sync clock {} [timezone:{:+}]", epoch_time, timezone));
    return Ok(());
}

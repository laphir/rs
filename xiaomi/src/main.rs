// How to use clap:
// https://github.com/clap-rs/clap/blob/master/examples/tutorial_derive/01_quick.rs
// https://docs.rs/clap/latest/clap/_derive/index.html
use clap::{Parser, Subcommand};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    time,
    sync::{Arc, Mutex},
    sync::mpsc::{Sender, Receiver},
    sync::mpsc,
};
use windows::{
    Devices::Bluetooth::Advertisement::{*},
    Foundation::TypedEventHandler
};
#[macro_use] extern crate prettytable;
use prettytable::Table;
use indicatif::{ProgressBar, ProgressStyle};
use console::{style, Emoji};

mod ble;
use ble::AdvertisementKind;
use xiaomi::{Config, DeviceConfig, format_bluetooth_address};

static CHECKBOX: Emoji<'_, '_> = Emoji("✅ ", "* ");
static TEMPERATURE: Emoji<'_, '_> = Emoji("🌡️", "Temp");
static HUMIDITY: Emoji<'_, '_> = Emoji("💧", "Humid");
static BATTERY: Emoji<'_, '_> = Emoji("🔋", "Batt");
static EXCLAMATION: Emoji<'_, '_> = Emoji("⚠️", "<!>");

#[derive(Parser)]
#[command(long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Show detailed messages
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Scan Xiaomi BLE devices
    Scan,
    /// Sync xiaomi clock devices
    Sync { name: Option<String> },

    /// Read toml file and print
    Toml,
}

fn main() -> Result<(), Box<dyn Error>>{
    let cli = Cli::parse();

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd
    match &cli.command {
        Commands::Scan => {
            scan(cli.verbose);
        },
        Commands::Sync { name } => {
            sync(cli.verbose, name);
        },
        Commands::Toml => {
            check_config();
        }
    }

    Ok(())
}

fn sync(_verbose: bool, _filter: &Option<String>) {
    // Load toml config file. This contains device name and timezone information.
    let config: Arc<Mutex<HashMap<u64, DeviceConfig>>> = Arc::new(Mutex::new(load_config()));
    // lock prevents destroying watcher object before completing event handler.
    let lock: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));
    // devices keeps the record of successfully synced devices. perhaps we can use HashSet instead.
    let devices: Arc<Mutex<HashSet<u64>>> = Arc::new(Mutex::new(HashSet::new()));
    // event handler runs in a background thread, so we don't print anything from there.
    // instead, log messages are transferred to main thread and printed along with a progress bar.
    let (tx, rx): (Sender<ble::SyncLogKind>, Receiver<ble::SyncLogKind>) = mpsc::channel();
    
    {
        let monitoring_period = 30;
        let spinner = ProgressBar::new_spinner();

        let config_clone = config.clone();
        let lock_clone = lock.clone();
        let devices_clone = devices.clone();
        let on_received = move |_sender: &Option<BluetoothLEAdvertisementWatcher>, args: &Option<BluetoothLEAdvertisementReceivedEventArgs>| {
            let mut _lifetime = lock_clone.lock().unwrap();
            ble::sync_device_args(&config_clone, &devices_clone, &tx, &args);
            Ok(())
        };

        let process_data = |wait: time::Duration| -> bool {
            match rx.recv_timeout(wait) {
                Err(_) => {
                    // Perhaps timeout. Do nothing.
                    return false;
                },
                Ok(data) => {
                    match data {
                        ble::SyncLogKind::Progress { address, log } => {
                            let mut device_name = format_bluetooth_address(address);
                            if let Some(device_config) = config.lock().unwrap().get(&address) {
                                if let Some(name) = &device_config.name {
                                    device_name = name.to_string();
                                }
                            }
    
                            spinner.println(format!("{}: {}", device_name, log));
                        },
                        ble::SyncLogKind::Error { address, log } => {
                            let mut device_name = format_bluetooth_address(address);
                            if let Some(device_config) = config.lock().unwrap().get(&address) {
                                if let Some(name) = &device_config.name {
                                    device_name = name.to_string();
                                }
                            }
    
                            spinner.println(format!("{}: {}", device_name, style(log).red()));
                        }
                    }
                    return true;
                }
            };
        };

        // initialize bluetooth watcher
        let watcher = BluetoothLEAdvertisementWatcher::new().expect("Creating BluetoothLEAdvertisementWatcher failed!");
        watcher.SetScanningMode(BluetoothLEScanningMode::Passive).expect("Changing ScanningMode failed");
        let token = watcher.Received(&TypedEventHandler::new(on_received)).unwrap();
    
        // start listening to advertisement.
        spinner.println(format!("Start monitoring BLE advertisement... {}", CHECKBOX));
        watcher.Start().expect("Starting BLE watcher failed");
        let start_time = time::Instant::now();

        spinner.enable_steady_tick(time::Duration::from_millis(120));
        spinner.set_style(
            ProgressStyle::with_template("{spinner:.red} {msg}")
                .unwrap()
                // For more spinners check out the cli-spinners project:
                // https://github.com/sindresorhus/cli-spinners/blob/master/spinners.json
                .tick_strings(&[
                    "▹▹▹▹▹",
                    "▸▹▹▹▹",
                    "▹▸▹▹▹",
                    "▹▹▸▹▹",
                    "▹▹▹▸▹",
                    "▹▹▹▹▸",
                    "▪▪▪▪▪",
                ]),
        );
        spinner.set_message("Listening...");

        // wait for messages
        while start_time.elapsed() < time::Duration::from_secs(monitoring_period) {
            process_data(time::Duration::from_millis(300));
        }

        // shutting down - remove the listener first.
        watcher.RemoveReceived(token).ok();

        // wait until existing event handler completes.
        spinner.println("Waiting worker thread complete...");
        spinner.set_message("Stopping...");
        let mut _lifetime = lock.lock().unwrap();
        while process_data(time::Duration::from_millis(0)) {}
        spinner.finish_and_clear();

        // stop the BLE watcher.
        watcher.Stop().expect("Stopping BLE watcher failed");
        spinner.println(format!("Stop monitoring BLE advertisement... {}", CHECKBOX));
        drop(watcher);
    }
}

// 'scan' command handler.
fn scan(_verbose: bool) {
    // Load toml config file. This contains device name and timezone information.
    let config = load_config();
    let (tx, rx): (Sender<ble::AdvertisementKind>, Receiver<ble::AdvertisementKind>) = mpsc::channel();
    let mut sensors: HashMap<u64, SensorData> = HashMap::new();

    // Watch on BLE advertisements
    {
        let monitoring_period = 10;
        let spinner = ProgressBar::new_spinner();
    
        let on_received = move |_sender: &Option<BluetoothLEAdvertisementWatcher>, args: &Option<BluetoothLEAdvertisementReceivedEventArgs>| {
            let value = ble::decode_advertisement(&args);
            match value {
                AdvertisementKind::Temperature(_) |
                AdvertisementKind::Humidity(_) |
                AdvertisementKind::Battery(_) => {
                    tx.send(value).unwrap();
                },
                _ => {}, // do nothing
            }
            Ok(())
        };
        let watcher = BluetoothLEAdvertisementWatcher::new().expect("Creating BluetoothLEAdvertisementWatcher failed!");
        watcher.SetScanningMode(BluetoothLEScanningMode::Passive).expect("Changing ScanningMode failed");
        let token = watcher.Received(&TypedEventHandler::new(on_received)).unwrap();
    
        // Start watcher and set the progress bar (spinner)
        watcher.Start().expect("Starting BLE watcher failed");
        spinner.enable_steady_tick(time::Duration::from_millis(120));
        spinner.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                // For more spinners check out the cli-spinners project:
                // https://github.com/sindresorhus/cli-spinners/blob/master/spinners.json
                .tick_strings(&[
                    "▹▹▹▹▹",
                    "▸▹▹▹▹",
                    "▹▸▹▹▹",
                    "▹▹▸▹▹",
                    "▹▹▹▸▹",
                    "▹▹▹▹▸",
                    "▪▪▪▪▪",
                ]),
        );
        spinner.println(format!("Start monitoring BLE advertisement... {}", CHECKBOX));
        spinner.set_message("Listening...");
        let start_time = time::Instant::now();

        let mut process_data = |wait: time::Duration| -> bool {
            match rx.recv_timeout(wait) {
                Err(_) => {
                    // Perhaps timeout. Do nothing.
                    return false;
                },
                Ok(data) => {
                    match &data {
                        AdvertisementKind::Temperature(value) | 
                        AdvertisementKind::Humidity(value) |
                        AdvertisementKind::Battery(value) => {
                            let mut name: String = format_bluetooth_address(value.address);
                            if let Some(device) = config.get(&value.address) {
                                if let Some(device_name) = &device.name {
                                    name = device_name.clone();
                                }
                            }
    
                            // create a new entry for this device, if it didn't exist.
                            if !sensors.contains_key(&value.address) {
                                sensors.insert(value.address, SensorData::new());
                            }
    
                            // Print the sensor value, and update sensor data.
                            if let AdvertisementKind::Temperature(_) = &data {
                                spinner.println(format!("{} - {} {} 'C", name, TEMPERATURE, value.value));
                                sensors.get_mut(&(value.address)).map(|val| val.set_temperature(value.value));
                            }
                            else if let AdvertisementKind::Humidity(_) = &data {
                                spinner.println(format!("{} - {} {} %", name, HUMIDITY, value.value));
                                sensors.get_mut(&(value.address)).map(|val| val.set_humidity(value.value));
                            }
                            else if let AdvertisementKind::Battery(_) = &data {
                                spinner.println(format!("{} - {} {} %", name, BATTERY, value.value));
                                sensors.get_mut(&(value.address)).map(|val| val.set_battery(value.value));
                            }
                        },
                        _ => {}, // do nothing
                    }
                    return true;
                }
            }
        };

        // Process transmitted messages
        while start_time.elapsed() < time::Duration::from_secs(monitoring_period) {
            process_data(time::Duration::from_millis(300));
        }

        // stop listening to the BLE advertisement, and handle all received data.
        watcher.RemoveReceived(token).ok();
        watcher.Stop().expect("Stopping BLE watcher failed");
        while process_data(time::Duration::from_millis(0)) {}

        spinner.println(format!("Stop monitoring BLE advertisement... {}", CHECKBOX));
        spinner.finish_and_clear();
        drop(watcher);
    }
    drop(rx); // done using channel.

    // This is for printing summary.
    println!("Summary:");
    let mut table = Table::new();
    table.add_row(row!["Device ID", "Temp.", "Humidity %", "Battery %"]);
    for (k, v) in sensors.iter() {
        let device_name: String;
        if let Some(d) = &config.get(k) {
            if let Some(n) = &d.name {
                device_name = n.clone();
            } else {
                device_name = format_bluetooth_address(*k);
            }
        } else {
            device_name = format_bluetooth_address(*k);
        };

        table.add_row(row![
            device_name,
            v.temperature.map_or("-".to_string(), |vv| vv.to_string()),
            v.humidity.map_or("-".to_string(), |vv| vv.to_string()),
            v.battery.map_or("-".to_string(), |vv| vv.to_string())]
        );
    }
    table.print_tty(true).ok();
}

struct SensorData {
    temperature: Option<f32>,
    humidity: Option<f32>,
    battery: Option<f32>
}

impl SensorData {
    pub fn new() -> Self {
        SensorData {humidity: None, temperature: None, battery: None}
    }

    pub fn set_temperature(&mut self, value: f32) {
        self.temperature = Some(value);
    }

    pub fn set_humidity(&mut self, value: f32) {
        self.humidity = Some(value);
    }

    pub fn set_battery(&mut self, value: f32) {
        self.battery = Some(value);
    }
}

fn check_config() {
    // get exe name of this process.
    let exe_path = std::env::current_exe().unwrap();
    let toml_name = std::path::Path::new(&exe_path).with_extension("toml");

    if !toml_name.exists() {
        eprintln!("{} Cannot find toml at {}", style("ERROR:").red(), toml_name.to_str().unwrap());
        eprintln!("Exe path: {:?}", exe_path);
        return;
    }
    println!("toml path: {}", style(toml_name.to_str().unwrap()).green());

    let content = std::fs::read_to_string(toml_name).unwrap();
    let config: Config = toml::from_str(&content).unwrap();

    {
        if let Some(devices) = config.devices {
            println!("Configuration:");
            let mut table = Table::new();
            table.add_row(row!["Address", "Name", "Omit", "Timezone", "Offset_Seconds"]);
            for device in devices {
                table.add_row(row![
                    format_bluetooth_address(device.address),
                    device.name.map_or("-".to_string(), |vv| vv.to_string()),
                    device.omit.map_or("-".to_string(), |vv| vv.to_string()),
                    device.timezone.map_or("-".to_string(), |vv| vv.to_string()),
                    device.offset_seconds.map_or("-".to_string(), |vv| vv.to_string()),
                ]);
            }
            table.print_tty(true).ok();
        }
        else {
            println!("{} No {} defined in toml.", EXCLAMATION, style("[[device]]").yellow());
        }
    }
}

fn load_config() -> HashMap<u64, DeviceConfig> {
    // get exe name of this process.
    let exe_path = std::env::current_exe().unwrap();
    let toml_name = std::path::Path::new(&exe_path).with_extension("toml");

    if !toml_name.exists() {
        return HashMap::new();
    }

    let content = std::fs::read_to_string(toml_name).unwrap();
    let decoded: Config = toml::from_str(&content).unwrap();
    let mut config: HashMap<u64, DeviceConfig> = HashMap::new();
    for c in decoded.devices.unwrap() {
        config.insert(c.address, c);
    }

    return config;
}

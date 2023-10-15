This software is compatible with Windows 11. It has been developed and tested on Windows 11.
There's no guarantee that it will function correctly on Windows 10.
Currently, there are no plans to support other operating systems.

## How to build

You need `rust` to compile this code. See following web site to install rust tool chain on your machine.
https://www.rust-lang.org/tools/install

In `xiaomi` folder, type following command to build. You need an internet connection.
```
cargo build --release
```

Once the build is successfully completed, you can find the `xiaomi.exe` in the following location. Copy this file to your preferred folder for command line tools.
`target\release\xiaomi.exe`

## How to use

Use `scan` command to discover xiaomi clock around you. In this example, it found my well known device "Tokyo".
```
d:\> xiaomi scan
Start monitoring BLE advertisement... ✅
Tokyo - 💧 68 %
Tokyo - 🔋 14 %
Tokyo - 💧 68 %
Stop monitoring BLE advertisement... ✅
Summary:
+-------------------+-------+------------+-----------+
| Device ID         | Temp. | Humidity % | Battery % |
+-------------------+-------+------------+-----------+
| AA:BB:CC:DD:EE:FF | -     | 68         | 14        |
+-------------------+-------+------------+-----------+
```

Create a toml file to give a human dreadable name to device. Create a `xiaomi.toml` along with `xiaomi.exe` file, need to place in a same folder.
```toml
[[devices]]
## address of the device. use : as a delimiter.
address = "AA:BB:CC:DD:EE:FF"
## the name of device.
name = "Tokyo"
## timezone of the time. "Asia/Seoul" is also possible.
## note that xiaomi device does not support timezones that don't fall on the hour.
## for example, Indian Standard Time is +05:30, which is not supported.
## in this case, use `offset_seconds` below.
timezone = "Asia/Tokyo"
## uncomment following line if you do not want to sync the device.
# omit = true
## sometimes you may want to set a clock 5 minutes ahead or 5 minutes behind.
## use +300 for 5 minutes ahead, -300 for 5 minutes behind.
# offset_seconds = +300

# define another device if you have more.
# [[devices]]
```

Then use `sync` command to sync the clock. Following is an example output of sync.
```
d:\> xiaomi sync
Start monitoring BLE advertisement... ✅
Tokyo: Connecting...
Tokyo: Querying service, UUID=ebe0ccb07a0a4b0c8a1a6ff2997da3a6
Tokyo: Querying characteristic, UUID=ebe0ccb77a0a4b0c8a1a6ff2997da3a6
Tokyo: Sync clock 1696891938 [timezone:+9]
Waiting worker thread complete...
Stop monitoring BLE advertisement... ✅
```

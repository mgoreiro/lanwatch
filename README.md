# lanwatch

[Español](README.es.md)

A native, minimal-footprint network monitor for the terminal, built for an Orange Pi or
Raspberry Pi (Linux ARM64). A single static binary of about 3 MB, with no dependencies and no daemons.

On an Orange Pi Zero 3 it uses **about 3.6 MB of RAM** and **about 1 % of one core** on average with the
interface open, including a full network sweep every minute.

## Tabs

| # | Tab | What it shows |
|---|---|---|
| 1 | **Devices** | Every machine on the LAN: IP, MAC, **vendor** (decoded from the MAC with the embedded IEEE database), name, **operating system** (identified by asking over mDNS, NetBIOS and UPnP, and estimated from ports and TTL), **open ports** and **inbound/outbound traffic** per device. `Enter` opens the detail view. |
| 2 | **Gateway** | Total inbound/outbound traffic of the router (rate, peak, total and graph) over SNMP; without SNMP, that of this machine's interface. |
| 3 | **DNS** | Compares latency and answers of the system DNS and of any IPs you add (`i`). 12 domains, first query and repeat (cache). |
| 4 | **Speed** | Latency, jitter, download and upload against public servers (LibreSpeed and Cloudflare). Pick one by hand or use **automatic mode** (`a`): the lowest latency. |
| 5 | **iPerf3** | A list of public iperf3 servers to run a test against (`Enter`), or **server** mode (default port 5201) so another machine can measure against this one. You can choose mode, port, target and direction; if the server or port is busy it offers to wait one minute and retry. Needs the `iperf3` program. |
| 6 | **WiFi** | Visible networks with signal quality (bar, dBm, channel, security) and, at the top, the current connection. `Enter` opens the network **detail** (AP vendor, channel, width, encryption, congestion). Without a connection there is no box; without WiFi it says "WiFi not available". Uses `nmcli` or `iw`. |
| 7 | **Help** | A guide to the app and, step by step, how to enable **NetFlow** and SNMP on your router (with this machine's real IP and the current status). The text comes from `docs/AYUDA.md` and `docs/EDGEROUTER.md`. |
| 8 | **About** | Project and author data (from `Cargo.toml`) and the state of this installation: executable, configuration, permissions, SNMP, NetFlow. |

**Optional programs** (the `.deb` recommends them): `iperf3` for the iPerf3 tab, and `nmcli` (NetworkManager) or `iw`
for the WiFi tab. Without them those tabs say so and everything else works as usual.

Global keys: `Tab` / `Shift+Tab` / `1`-`8` switch tabs, `q` quits. Each tab shows its own keys at the bottom.

## Install

**Debian / Ubuntu / Raspberry Pi OS package (recommended)**, available for `arm64` and `amd64`. Download the `.deb` from the
[latest release](https://github.com/mgoreiro/lanwatch/releases/latest) and install it:

```bash
sudo apt install ./lanwatch_0.4.0_arm64.deb     # installs the binary, the man page and the documentation
lanwatch
```

The package gives the binary `CAP_NET_RAW` (with `setcap`, recommended through `libcap2-bin`) on every install and
**upgrade**, so there is nothing to repeat. Remove it with `sudo apt remove lanwatch`; your configuration
(`~/.config/lanwatch.conf`) is left alone.

To build the packages (needs Docker): `./scripts/build-deb.sh [arm64|amd64|all] [--lint]` creates `dist/lanwatch_<version>_<arch>.deb`.

**Standalone binary:** run `./scripts/build-aarch64.sh` and copy `dist/lanwatch-aarch64` wherever you like. For the TTL,
press `c` or use `--setcap`.

## Usage

```bash
./lanwatch                                  # full interface
./lanwatch --once                           # no interface: scan, print the table and exit (scripts/cron)
./lanwatch --snmp public/eth0               # router counters over SNMP (host = gateway)
./lanwatch --netflow 2055 --iface end0      # NetFlow collector and interface selection
./lanwatch --help
```

Options can also go in `~/.config/lanwatch.conf` or `/etc/lanwatch.conf` (`key=value`, without `--`).

### What each feature needs

| Feature | Needs |
|---|---|
| Devices, vendor, ports | Nothing: no root. UDP sweep + the kernel ARP table + TCP *connect* scan + mDNS/NetBIOS/SSDP probes. With `CAP_NET_RAW`, **its own ARP** (exact presence). |
| TTL for better OS detection | `CAP_NET_RAW` on the binary: press **`c`** in the Devices tab (it asks for confirmation, then your `sudo` password, and the app restarts itself) or run `./lanwatch --setcap`. If you copy the binary again you must repeat it. Without it the OS is estimated from ports, vendor and name. |
| Per-device traffic | The router must export **NetFlow (v5, v9 or IPFIX)** to this machine. See [docs/EDGEROUTER.md](docs/EDGEROUTER.md). |
| Gateway traffic | SNMP v2c enabled on the router. Set it up from the tab (key `s`) or with `--snmp`. |
| DNS, speed | Internet access. |

Without the router configuration, everything else works and the per-device traffic columns show `n/a`.

The in-app help and the router guides (`docs/AYUDA.md`, `docs/EDGEROUTER.md`) are currently in Spanish.

## Build

```bash
cargo run                         # on any Linux/macOS (discovery uses /proc: Linux only)
cargo test                        # unit tests
cargo test -- --ignored --nocapture   # tests that use the network (DNS, server list and latency)
./scripts/build-aarch64.sh        # static binary for Orange Pi / 64-bit Raspberry Pi, through Docker
./scripts/build-deb.sh            # .deb packages (arm64 and amd64)
scp dist/lanwatch-aarch64 user@pi:~/lanwatch
```

## Known limits

- **Device presence:** with `CAP_NET_RAW` it is exact (own ARP). Without it, it relies on the kernel ARP table, which can keep a switched-off machine for a few minutes. In both cases a device is marked offline after 2 consecutive sweeps without an answer.
- **Several IPs with the same MAC** (proxy ARP, macvlan containers) appear as different devices with the same MAC.
- **OS:** it is an identification from what the device says about itself (mDNS, NetBIOS, UPnP) plus heuristics, not an exact fingerprint. The random MACs of modern phones have no vendor.
- **NetFlow:** it is exported when the flow expires, so rates are 60-second averages, not instantaneous. Only IPv4 is counted.
- **Speed tests:** public servers rate-limit heavy users (they answer 403/429). If that happens the app says so; just pick another server or wait.

## Structure and extending

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) (in Spanish): how to add a tab, a data source or a NetFlow parser.

## License

GPL-3.0 or later. See [LICENSE](LICENSE).

**Author:** Miguel González Oreiro · [github.com/mgoreiro](https://github.com/mgoreiro)

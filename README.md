<div align="center">

# AirCard · Windows 🎴

**Apple Wallet card skinner & lock-screen passcode themer for iOS 18+ — no jailbreak.**

Native Windows client written in Rust, powered by the `airlift` AirTraffic sync conduit.

<p>
  <img alt="platform" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0a7bbb">
  <img alt="language" src="https://img.shields.io/badge/built%20with-Rust%202024-dea584">
  <img alt="ui" src="https://img.shields.io/badge/UI-egui%20%2F%20eframe-7c5cff">
  <img alt="ios" src="https://img.shields.io/badge/iOS-18.0%20–%2027.0.1-000000">
  <img alt="license" src="https://img.shields.io/badge/license-MIT-green">
</p>

</div>

---

> [!WARNING]
> **Cosmetic only. Personal use.** AirCard rewrites the *local artwork* of a Wallet card face and
> the lock-screen keypad assets. It does **not** touch the Apple Pay payment applet, token, or
> balance — tap-to-pay keeps working exactly as before. Personal fork, not affiliated with Apple Inc.

## Contents

- [Compatibility](#compatibility)
- [Features](#features)
- [How it works](#how-it-works)
- [Requirements](#requirements)
- [Install](#install)
- [Usage](#usage)
- [Restore / going back to the original](#restore--going-back-to-the-original)
- [Troubleshooting](#troubleshooting)
- [FAQ](#faq)
- [Project layout](#project-layout)
- [Credits & license](#credits--license)

---

## Compatibility

| iOS version | Status | Notes |
| :--- | :--- | :--- |
| **iOS 18.0 – 27.0.1** | ✅ Supported | Wallet skins + lock-screen passcode themes |
| **iOS 27.2 beta 1–2** | ✅ Supported | Working |
| **iOS 27.2 beta 3+** | ❌ Patched | Apple closed the `airlift` / AirTraffic exploit |

> [!IMPORTANT]
> Do **not** update to iOS 27.2 beta 3 or newer if you want to keep using AirCard.

## Features

- 🎨 **Custom card skins** — assign your own artwork to Apple Pay / Apple Cash card faces.
- 🔢 **Lock-screen passcode themes** — apply Cowabunga / Nugget `.passthm` keypad art.
- 📱 **Live card detection** — tap a card in Wallet and its hash is captured via `syslog_relay`.
- 💾 **Reversible** — the original card face is backed up on first apply (see [Restore](#restore--going-back-to-the-original)).
- 📶 **USB & Wi-Fi transport** — pair once over USB, then sync over the local network.
- ⚡ **Native & light** — a single `aircard.exe`, no Python / webview runtimes.
- 🌐 **English / Simplified Chinese** UI, remembered across launches.
- 🧱 **Clean Material-3 interface** built with `egui` / `eframe`.

## How it works

AirCard talks to the iPhone exactly like iTunes does, through Apple's own libraries:

1. **Device discovery** — connects to the local `usbmuxd` service (`127.0.0.1:27015`) and loads
   `CoreFoundation.dll` / `MobileDevice.dll` / `AirTrafficHost.dll` to enumerate paired devices over
   USB or Wi-Fi.
2. **Card detection** — opens `com.apple.syslog_relay` and watches for the card hash as you tap a
   card in Wallet.
3. **Asset injection** — artwork is written into `…/Library/Passes/Cards/<hash>.pkpass` using the
   `airlift` **AirTraffic / Books sync** conduit (a streaming-zip staged through
   `MobileInstallation`), which reaches the Wallet directory without a jailbreak. The Books state is
   snapshotted and restored around every write.
4. **Cache invalidation** — the `.cache` / `.pkcache` leaves are corrupted so Wallet regenerates the
   preview from the new artwork.

## Requirements

- **Windows 10 / 11 (64-bit)**
- **Apple Mobile Device Support DLLs** — `CoreFoundation.dll`, `MobileDevice.dll`, `AirTrafficHost.dll`.
- A Lightning / USB-C cable for the first trust + pairing.

> [!CAUTION]
> **The Microsoft Store "iTunes" / "Apple Devices" build is not enough by itself.**
> Those packages keep their DLLs inside a sandboxed `WindowsApps` folder that third-party apps
> **cannot load or copy** (access denied), so AirCard can detect the drivers yet fail to start.

<details>
<summary><b>Getting the DLLs (one-time)</b></summary>

Install the **desktop 64-bit iTunes from apple.com** (not the Microsoft Store build):
<https://www.apple.com/itunes/download/win64>. It installs the DLLs into
`C:\Program Files\Common Files\Apple\Mobile Device Support` with normal permissions.

If the Win32 installer refuses because a Store build is present, extract `iTunes64Setup.exe`
with 7-Zip and run only `AppleMobileDeviceSupport64.msi` + `AppleApplicationSupport64.msi`.

**Where AirCard looks**, first match wins:

1. `C:\Program Files\Common Files\Apple\Mobile Device Support`
2. `C:\Program Files (x86)\Common Files\Apple\Mobile Device Support`
3. Next to `aircard.exe` (or a `MobileDeviceSupport` folder beside it)
4. `%LOCALAPPDATA%\AirCard\MobileDeviceSupport`
5. The live Apple daemon's directory (discovered from the running `AppleMobileDeviceProcess`)

</details>

## Install

### Quick install (recommended)

From the repo root, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

The script builds `aircard.exe` (or reuses an existing release build), installs it to
`%LOCALAPPDATA%\Programs\AirCard`, creates Desktop + Start Menu shortcuts, and checks the Apple DLL
prerequisite — opening the iTunes download page if the DLLs are missing. Uninstall with
`install.ps1 -Uninstall`.

> No Rust toolchain? Grab `aircard.exe` from the repo **Releases**, drop it in `target\release\`, then
> run the script — or just run the exe directly.

### Prebuilt releases

Pushing a `v*` tag runs the **Release** GitHub Action, which builds `aircard.exe` and attaches it to
a GitHub Release:

```bash
git tag v1.2.4 && git push origin v1.2.4
```

### Build from source

```bash
cargo build --release   # -> target/release/aircard.exe
```

Requires a recent stable Rust toolchain (`edition = 2024`).

## Usage

### Customize a Wallet card

1. Connect the iPhone (USB or paired Wi-Fi) and unlock it.
2. On the **Wallet** tab, click **Scan**, then open Wallet on the iPhone and tap the target card.
   Its hash is captured automatically — click **Stop**.
3. Click **Choose Image…** (PNG / JPG / WebP). Drag inside the preview to position the crop.
4. Click **Apply Card Skin**.
5. Force-close Wallet on the iPhone (App Switcher → swipe away) and reopen it to see the new face.

### Apply a lock-screen passcode theme

1. Switch to the **Passcode** tab → **Choose .passthm…**.
2. Pick the iOS cache target (Auto / TelephonyUI-10 for iOS 18+).
3. Click **Apply Passcode Theme**, then lock the screen or open the dialer to see the keypad.

### Where to find card art

| Source | What |
| :--- | :--- |
| [cardart.cc](https://cardart.cc) | Ready-made 1536×969 PNG card faces |
| [aircards.org](https://aircards.org) | Community card-art gallery |
| [Unsplash](https://unsplash.com) · [Pexels](https://pexels.com) | High-res images — crop to a **1.585** ratio |

## Restore / going back to the original

The **Restore Original** button replays the original card face that AirCard backs up the **first time
you apply a skin**. It stays greyed out until such a backup exists.

> [!NOTE]
> On a non-jailbroken device the capture reads through the media-jailed `com.apple.afc` service, which
> cannot see `…/Library/Passes/Cards`, so the original may not be captured and Restore can stay greyed.
> **Reliable way back:** remove the card from Wallet and re-add it — Apple re-downloads the official
> issuer artwork. Payment re-provisions normally (the bank may send a verification code).

## Troubleshooting

<details>
<summary><b>iPhone not detected</b></summary>

- Unlock the iPhone and tap **Trust This Computer** (reconnect the cable if the prompt is missing).
- Confirm the **Apple DLLs** are reachable — see [Requirements](#requirements). The #1 cause is having
  only the Microsoft Store build.
- Corrupt Apple USB drivers? Install [3uTools](https://www.3u.com) → **Toolbox → Repair Driver**,
  reconnect, and relaunch.
- Click **Refresh** in AirCard after reconnecting.

</details>

<details>
<summary><b>Wi-Fi device won't connect</b></summary>

Pair once over **USB** and trust the computer, enable *Sync over Wi-Fi* in Apple Devices / iTunes,
keep both on the same network, then pick **WiFi only** and **Refresh**.

</details>

<details>
<summary><b>Skin applied but Wallet still shows the old face</b></summary>

Force-close Wallet from the App Switcher and reopen it; if it persists, restart the iPhone.

</details>

## Project layout

| Module | Role |
| :--- | :--- |
| `apple.rs` | Loads Apple DLLs (CoreFoundation / MobileDevice / AirTrafficHost), locates the support dir |
| `device.rs` | `usbmuxd` device enumeration, pairing, sessions, transport selection |
| `afc.rs` | AFC file-system client on the device |
| `airlift.rs` / `airtraffic.rs` | Streaming-zip staging and the AirTraffic sync conduit |
| `scanner.rs` | `syslog_relay` card-hash detection |
| `wallet_backup.rs` | Capture / restore of the original card artwork |
| `image_skin.rs` | Crop & scale artwork to 1536×969 |
| `passthm.rs` / `flasher.rs` | `.passthm` parsing and the write/flash pipeline |
| `app.rs` / `i18n.rs` | egui interface and localization |

## Credits & license

- Original project: [Lumid-Off/AirCard-Windows](https://github.com/Lumid-Off/AirCard-Windows) and
  [Mak5er/AirCard](https://github.com/Mak5er/AirCard).
- `airlift` AirTraffic sync technique and the Cowabunga / Nugget theme ecosystems.

Released under the **MIT License** — see [LICENSE](LICENSE).

# phone-upload

Send files from your computer to your phone over the local network. No cloud, no cables, no pairing — the phone app advertises itself via mDNS and the CLI finds it automatically.

- `app/` — Flutter app ("Phone Drop") that runs on the phone. Advertises `_phoneupload._tcp` via mDNS (Bonjour/NSD), runs an HTTP server on a random port, saves incoming files, and lists them with a share button.
- `gui/` — Rust desktop GUI (egui) that discovers the phone via mDNS and streams files to it.

## Usage

1. Open Phone Drop on the phone (same Wi-Fi network as the computer). The status card shows "Discoverable on your network".
2. On the computer, run `phone-upload`. The window shows the discovered phone in the status bar (it browses `_phoneupload._tcp.local.` continuously; IPv4 preferred, scoped link-local IPv6 as a fallback). Drag files onto the window — or click "Choose files…" — and each uploads with a progress bar. Duplicate names are saved on the phone as `name (1).ext`, etc.

3. Files land in the app's Downloads directory (path shown in the app). Use the share button to move them into Photos, Drive, etc.

## Build

```sh
# Desktop GUI
cd gui && cargo build --release   # binary at target/release/phone-upload

# App
cd app && flutter build apk --release   # or: flutter run
```

## Git flow

The repo follows [git-flow](https://nvie.com/posts/a-successful-git-branching-model/):

- `main` — release history only; every commit is a tagged release.
- `develop` — integration branch and the repo default; day-to-day work lands here.
- `feature/<name>` — branched from and merged back into `develop`.
- `release/<version>` — branched from `develop`; version bumps and fixes only, merged into `main` (tagged `vX.Y.Z`) and back into `develop`.
- `hotfix/<version>` — branched from `main` for urgent fixes, merged into both `main` and `develop`.

CI (`.github/workflows/build.yml`) runs tests and builds the desktop binaries (Linux/Windows/macOS) and the Android APK on every push and PR to `main`/`develop`, and attaches all of them to a GitHub Release when a `v*` tag is pushed.

## Protocol

One request per file, plain HTTP on the advertised port:

```
PUT /?name=<urlencoded filename>
Content-Length: <bytes>

<raw file bytes>
```

`200` means saved. The server keeps only the basename of `name`, so a path can't escape the save directory.

**Security note:** there is no auth or TLS — anyone on your LAN can send files to the phone while the app is open. Fine for a home network; don't run it on hostile Wi-Fi.

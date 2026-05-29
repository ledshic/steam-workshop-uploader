# Steam Workshop Uploader

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A cross-platform desktop app (Windows / macOS / Linux) built with **Tauri v2 + Rust + Svelte 5** for uploading mods to the Steam Workshop using the Steamworks SDK by default, with `steamcmd` available as a fallback.

## Features

- General-purpose uploader (any Steam AppID)
- Clean form for title, description, preview, tags, visibility
- Live streaming console output from Steamworks SDK or steamcmd
- One-click upload with native Steamworks SDK
- Optional VDF generation + steamcmd upload fallback
- Auto-detect + manual path for steamcmd fallback
- Quick presets for popular games (Rust, Arma 3, DayZ, etc.)
- Works for both new uploads and updates (publishedfileid)

## Prerequisites

1. **Rust** (installed via rustup)
2. **Node.js 18+** + pnpm (or npm)
3. **Steam client** running and logged in for the default SDK upload method
4. Optional: **steamcmd** for the fallback upload method
5. On Windows, make sure `steam_api64.dll` from the Steamworks redistributables is available through `STEAMWORKS_SDK_PATH` or a local Steamworks SDK checkout so the bundled app can ship it

## Development

```bash
cd steam-workshop-uploader
pnpm install
pnpm tauri dev
```

The first run will download Tauri dependencies.

## Production Build

```bash
pnpm tauri build
```

The bundled app will be in `src-tauri/target/release/bundle/`.

## How to Use the App

1. **Default SDK upload**
   - Start Steam and log into the account that has Workshop permissions for the target app
   - In this app, keep **Steamworks SDK** selected in Settings
   - Fill the upload form and click **Upload with Steamworks SDK**

2. **Fallback steamcmd upload**
   - Switch Settings to **steamcmd**
   - Set the path to `steamcmd` or use Auto-detect
   - Click **Generate VDF** if you want to preview the config
   - Click **Upload with steamcmd**

3. **First time steamcmd login (required once for fallback)**
   - Open Terminal / CMD
   - Run: `steamcmd +login YOUR_STEAM_USERNAME`
   - Enter password + Steam Guard code if prompted
   - Exit steamcmd

Watch the live output in the console. On SDK success, the app stores the new/updated PublishedFileID in the form.

## Architecture Notes

- Steamworks SDK uploads use ISteamUGC create/update/submit calls in Rust (`src-tauri/src/steamworks.rs`)
- VDF generation and steamcmd spawning happen in Rust (`src-tauri/src/lib.rs`)
- Uses Tauri v2 plugins: dialog, fs, shell, store
- The default upload path uses the running Steam client session; steamcmd remains available as a manual fallback

## Future Improvements

- Bundle or auto-download steamcmd per platform
- Saved upload profiles / history using Tauri Store
- More detailed Steamworks SDK progress and legal agreement handling

Built as a general-purpose tool for the modding community.

---

Originally scaffolded from the official Tauri + Svelte template.

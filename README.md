# Steam Workshop Uploader

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A cross-platform desktop app (Windows / macOS / Linux) built with **Tauri v2 + Rust + Svelte 5** for uploading mods to the Steam Workshop using `steamcmd`.

## Features

- General-purpose uploader (any Steam AppID)
- Clean form for title, description, preview, tags, visibility
- Live streaming console output from steamcmd
- One-click VDF generation + upload
- Auto-detect + manual path for steamcmd
- Quick presets for popular games (Rust, Arma 3, DayZ, etc.)
- Works for both new uploads and updates (publishedfileid)

## Prerequisites

1. **Rust** (installed via rustup)
2. **Node.js 18+** + pnpm (or npm)
3. **steamcmd** (the app helps you locate or you can download it from Valve)

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

1. **First time Steam login (required once)**
   - Open Terminal / CMD
   - Run: `steamcmd +login YOUR_STEAM_USERNAME`
   - Enter password + Steam Guard code if prompted
   - Exit steamcmd

2. In this app:
   - Set the path to `steamcmd` (or use Auto-detect)
   - Fill the upload form (AppID, content folder, title, etc.)
   - Click **Generate VDF** (preview the config)
   - Click **Upload to Workshop**

3. Watch the live output. On success you will see the new/updated PublishedFileID.

## Architecture Notes

- VDF generation and steamcmd spawning happen in Rust (`src-tauri/src/lib.rs`)
- Uses Tauri v2 plugins: dialog, fs, shell, store
- Uploads use the standard `steamcmd +workshop_build_item "file.vdf" +quit` method (most reliable for third-party tools)

## Future Improvements

- In-app Steam Guard code input (advanced bidirectional process control)
- Bundle or auto-download steamcmd per platform
- Saved upload profiles / history using Tauri Store
- Native Steamworks SDK path (optional, for users who have the SDK)

Built as a general-purpose tool for the modding community.

---

Originally scaffolded from the official Tauri + Svelte template.

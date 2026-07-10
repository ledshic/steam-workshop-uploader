# Steam Workshop Uploader (RimWorld)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A cross-platform desktop app (Windows / macOS / Linux) built with **Tauri v2 + Rust + Svelte 5** for uploading **RimWorld** mods to the Steam Workshop. Uses the Steamworks SDK by default, with `steamcmd` as a fallback.

## Features

- **RimWorld-first workflow** (AppID `294100`)
- **Auto-detect mod structure** from `About/About.xml`
  - Title from `<name>`
  - Workshop description from `<description>`
  - Tags from `Mod` + `<supportedVersions>`
  - Cover/preview from `About/Preview.png` (or `.jpg` / `ModIcon.png`)
  - Update ID from `About/PublishedFileId.txt`
- **Temp upload package**: after selecting a mod, one-click generate a clean temp folder (excludes `Source/`, `.git/`, `bin/`, `obj/`, `.sln`, `.csproj`, …) used for upload
- **Preview package** in the system file manager (Finder / Explorer)
- **One-click upload / update** — pick folder → detect → temp package → upload
- Writes `About/PublishedFileId.txt` after a successful first upload so the next run is an update
- Live console output from Steamworks SDK or steamcmd
- Optional VDF generation + steamcmd fallback
- Description-only / preview-only update helpers (SDK mode)

## Prerequisites

1. **Rust** (via rustup)
2. **Node.js 18+** + pnpm (or npm)
3. **Steam client** running and logged in (for SDK upload)
4. Optional: **steamcmd** for the fallback upload method
5. On Windows, make sure `steam_api64.dll` from the Steamworks redistributables is available through `STEAMWORKS_SDK_PATH` or a local Steamworks SDK checkout

## Development

```bash
cd steam-workshop-uploader
pnpm install
pnpm tauri dev
```

## Production Build

```bash
pnpm tauri build
```

Bundled app output: `src-tauri/target/release/bundle/`.

## How to Use (RimWorld)

### One-click path

1. Start Steam and log into the account that owns the Workshop item (or has permission to publish)
2. Keep **Steamworks SDK** selected in Settings
3. Click **One-click RimWorld upload / update**
4. Select your mod root folder (the folder that contains `About/`)
5. The app:
   - Parses `About/About.xml` for title & description
   - Picks `About/Preview.png` as the Workshop cover
   - Reads `About/PublishedFileId.txt` if present (update) or creates a new item
   - Optionally builds a clean package (default on)
   - Uploads / updates via Steamworks SDK
   - Writes `PublishedFileId.txt` after a successful new upload

### Manual path

1. Browse the mod folder (auto-fill still runs)
2. Edit title / description / tags if needed
3. Click **Upload form as-is**

### steamcmd fallback

1. Settings → **steamcmd**
2. Set path or Auto-detect
3. `Generate workshop.vdf` then **Upload form as-is**
4. First-time login: `steamcmd +login YOUR_STEAM_USERNAME`

## Expected mod layout

```
MyMod/
  About/
    About.xml            # required — name, description, versions
    Preview.png          # preferred Workshop cover
    PublishedFileId.txt  # optional — present after first upload
    ModIcon.png          # fallback cover if Preview is missing
  Assemblies/ …
  Defs/ …
  …
```

Clean packaging keeps Workshop content lean by skipping developer folders such as `Source/`, `.git/`, `bin/`, `obj/`.

## Architecture Notes

- RimWorld detection & packaging: `src-tauri/src/rimworld.rs`
- Steamworks SDK uploads: `src-tauri/src/steamworks.rs`
- VDF generation + steamcmd spawn: `src-tauri/src/lib.rs`
- UI: `src/routes/+page.svelte`
- Tauri v2 plugins: dialog, fs, shell, store

## Future Improvements

- Bundle or auto-download steamcmd per platform
- Saved upload profiles / history using Tauri Store
- More detailed Steamworks SDK progress and legal agreement handling

Built as a RimWorld-focused tool for the modding community.

---

Originally scaffolded from the official Tauri + Svelte template.

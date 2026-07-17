# Steam Workshop Uploader (RimWorld + Bannerlord)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A cross-platform desktop app (Windows / macOS / Linux) built with **Tauri v2 + Rust + Svelte 5** for uploading **RimWorld** and **Bannerlord** mods to the Steam Workshop. Uses the Steamworks SDK by default, with `steamcmd` as a fallback.

## Features

- **Game presets**
  - RimWorld — AppID `294100`
  - Bannerlord — AppID `261550`
- **RimWorld auto-detect** from `About/About.xml`
  - Title / description / versions / `Preview.png` / `PublishedFileId.txt`
- **Bannerlord auto-detect** from `SubModule.xml` (and common repo layouts)
  - Prefers ship-ready `out/ModuleId/` or module root with `SubModule.xml`
  - Name / Id / Version / dependencies / README description
  - Preview from `Image.png` or `_Workshop/*.png`
  - Workshop id from `WorkshopItemId.txt` or `WorkshopUpdate.xml` ItemId
- **Temp upload package** + open in system file manager
- **One-click upload / update**
- **Automatic localized descriptions** (SDK) — uploads `About/About.xml` as English, then applies each `About/About.<locale>.xml` description (for example `About.zh-CN.xml` → Simplified Chinese)
- **Easy / Advanced UI** (default Advanced) — Easy is game → drop folder → one-click upload
- Light / dark / system Dock icons (Settings)
- Live console, steamcmd fallback, description/preview-only updates

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

## How to Use

1. Start Steam and log in
2. Pick **RimWorld** or **Bannerlord** in the Game selector
3. **One-click upload / update** → select the mod/module folder
4. Or: browse folder → **Generate temp package** → preview in Finder → upload

### steamcmd fallback

1. Settings → **steamcmd**
2. Set path or Auto-detect
3. `Generate workshop.vdf` then **Upload form as-is**
4. First-time login: `steamcmd +login YOUR_STEAM_USERNAME`

## Expected layouts

### RimWorld

```
MyMod/
  About/
    About.xml
    Preview.png
    PublishedFileId.txt   # optional
  Assemblies/ Defs/ …
```

### Bannerlord (rules from workspace modules)

**Ship-ready module (preferred upload content):**

```
ModuleId/                         # e.g. out/Bannerlord.AutoAmmoPickup/
  SubModule.xml                   # required — Name / Id / Version / deps
  bin/Win64_Shipping_Client/      # required for code mods — *.dll
  ModuleData/                     # optional — languages, XML
  GUI/ AssetPackages/ …           # optional assets
```

**Common repo layouts (detected, not all upload-ready as-is):**

```
Repo/
  out/ModuleId/          # preferred build output → selected automatically
  _Module/               # template / partial module
  _Workshop/             # previews + WorkshopUpdate.xml (not module content)
  dev/ src/ .git/        # excluded from clean package
```

**SubModule.xml fields used:**

| Field | Use |
|-------|-----|
| `<Name value="…"/>` | Workshop title |
| `<Id value="…"/>` | Module id |
| `<Version value="v…"/>` | Change note / tags |
| `<DependedModule Id="…"/>` | Listed in description |
| Singleplayer / Multiplayer | Tags |

**Workshop id sources:** `WorkshopItemId.txt`, `PublishedFileId.txt`, or `_Workshop/WorkshopUpdate.xml` → `<ItemId Value="…"/>`.

**Preview sources:** module `Image.png` / `Preview.png`, else first suitable `_Workshop/*.png`.

Clean packaging **keeps** `bin/Win64_Shipping_Client`, and **drops** `src/`, `dev/`, `.git/`, `.pdb`, `.cs`, `_Workshop/`, project files.

## Architecture Notes

- RimWorld: `src-tauri/src/rimworld.rs`
- Bannerlord: `src-tauri/src/bannerlord.rs`
- Steamworks SDK: `src-tauri/src/steamworks.rs`
- Dock icons: `src-tauri/src/app_icon.rs`
- UI: `src/routes/+page.svelte`

## Future Improvements

- Bundle or auto-download steamcmd per platform
- Saved upload profiles / history using Tauri Store
- More detailed Steamworks SDK progress and legal agreement handling

Built as a RimWorld-focused tool for the modding community.

---

Originally scaffolded from the official Tauri + Svelte template.

fn main() {
    copy_steam_api_dylib();
    tauri_build::build()
}

#[cfg(target_os = "macos")]
fn copy_steam_api_dylib() {
    use std::{env, fs, path::PathBuf};

    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = match env::var("OUT_DIR") {
        Ok(value) => PathBuf::from(value),
        Err(_) => return,
    };

    let Some(profile_dir) = out_dir
        .ancestors()
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name == "debug" || name == "release")
        })
        .map(PathBuf::from)
    else {
        return;
    };

    let build_dir = profile_dir.join("build");
    let Ok(entries) = fs::read_dir(&build_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path().join("out").join("libsteam_api.dylib");
        if path.exists() && steam_api_dylib_supports_current_crate(&path) {
            let destination = profile_dir.join("libsteam_api.dylib");
            if let Err(err) = fs::copy(&path, &destination) {
                println!(
                    "cargo:warning=failed to copy libsteam_api.dylib from {} to {}: {}",
                    path.display(),
                    destination.display(),
                    err
                );
            }
            if let Some(target_dir) = profile_dir
                .ancestors()
                .find(|path| path.file_name().is_some_and(|name| name == "target"))
            {
                let tauri_bundle_dir = target_dir.join("release");
                let tauri_bundle_path = tauri_bundle_dir.join("libsteam_api.dylib");
                if let Err(err) = fs::create_dir_all(&tauri_bundle_dir) {
                    println!(
                        "cargo:warning=failed to create {}: {}",
                        tauri_bundle_dir.display(),
                        err
                    );
                } else if let Err(err) = fs::copy(&path, &tauri_bundle_path) {
                    println!(
                        "cargo:warning=failed to copy libsteam_api.dylib from {} to {}: {}",
                        path.display(),
                        tauri_bundle_path.display(),
                        err
                    );
                }
            }
            return;
        }
    }

    println!(
        "cargo:warning=compatible libsteam_api.dylib was not found under {}",
        build_dir.display()
    );
}

#[cfg(target_os = "macos")]
fn steam_api_dylib_supports_current_crate(path: &std::path::Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| {
        bytes
            .windows(b"SteamAPI_InitFlat".len())
            .any(|window| window == b"SteamAPI_InitFlat")
    })
}

#[cfg(not(target_os = "macos"))]
fn copy_steam_api_dylib() {}

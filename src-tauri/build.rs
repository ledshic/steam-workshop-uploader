fn main() {
    copy_steam_api_dylib();
    copy_steam_api_windows_dll();
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

#[cfg(target_os = "windows")]
fn copy_steam_api_windows_dll() {
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

    let Some(source_path) = find_windows_steam_api_dll(&profile_dir) else {
        println!(
            "cargo:warning=compatible steam_api64.dll was not found under {}",
            profile_dir.join("build").display()
        );
        return;
    };

    let destination = profile_dir.join("steam_api64.dll");
    if let Err(err) = fs::copy(&source_path, &destination) {
        println!(
            "cargo:warning=failed to copy steam_api64.dll from {} to {}: {}",
            source_path.display(),
            destination.display(),
            err
        );
    }

    if let Some(target_dir) = profile_dir
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "target"))
    {
        let tauri_bundle_dir = target_dir.join("release");
        let tauri_bundle_path = tauri_bundle_dir.join("steam_api64.dll");
        if let Err(err) = fs::create_dir_all(&tauri_bundle_dir) {
            println!(
                "cargo:warning=failed to create {}: {}",
                tauri_bundle_dir.display(),
                err
            );
        } else if let Err(err) = fs::copy(&source_path, &tauri_bundle_path) {
            println!(
                "cargo:warning=failed to copy steam_api64.dll from {} to {}: {}",
                source_path.display(),
                tauri_bundle_path.display(),
                err
            );
        }
    }
}

#[cfg(target_os = "windows")]
fn find_windows_steam_api_dll(profile_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    use std::{env, fs, path::PathBuf};

    let build_dir = profile_dir.join("build");
    let candidate_names = ["steam_api64.dll", "steam_api.dll"];

    if let Ok(entries) = fs::read_dir(&build_dir) {
        for entry in entries.flatten() {
            for candidate_name in candidate_names {
                let path = entry.path().join("out").join(candidate_name);
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }

    let sdk_roots = [
        env::var_os("STEAMWORKS_SDK_PATH"),
        env::var_os("STEAMWORKS_SDK"),
        env::var_os("STEAMWORKS_SDK_REDIST"),
        env::var_os("STEAMWORKS_REDIST_PATH"),
        env::var_os("STEAMWORKS_REDIST"),
    ];

    let sdk_relative_candidates = [
        "redistributable_bin/win64/steam_api64.dll",
        "redistributable_bin/steam_api64.dll",
        "redistributable_bin/win32/steam_api.dll",
    ];

    for root in sdk_roots.into_iter().flatten() {
        let root = PathBuf::from(root);
        for relative_path in sdk_relative_candidates {
            let path = root.join(relative_path);
            if path.exists() {
                return Some(path);
            }
        }
    }

    None
}

#[cfg(not(target_os = "windows"))]
fn copy_steam_api_windows_dll() {}

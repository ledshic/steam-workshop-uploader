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
        if path.exists() {
            let destination = profile_dir.join("libsteam_api.dylib");
            if let Err(err) = fs::copy(&path, &destination) {
                println!(
                    "cargo:warning=failed to copy libsteam_api.dylib from {} to {}: {}",
                    path.display(),
                    destination.display(),
                    err
                );
            }
            return;
        }
    }

    println!(
        "cargo:warning=libsteam_api.dylib was not found under {}",
        build_dir.display()
    );
}

#[cfg(not(target_os = "macos"))]
fn copy_steam_api_dylib() {}

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tauri_plugin_shell::{process::CommandEvent, ShellExt};

#[cfg(feature = "steamworks-sdk")]
mod steamworks;

const STEAM_STATUS_PROBE_ARG: &str = "--steam-status-probe";
const STEAM_STATUS_PROBE_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopItem {
    pub app_id: u32,
    pub published_file_id: Option<u64>,
    pub content_folder: String,
    pub preview_file: Option<String>,
    pub title: String,
    pub description: String,
    pub change_note: Option<String>,
    pub visibility: u8,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub published_file_id: u64,
    pub needs_legal_agreement: bool,
    pub method: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DescriptionUpdateRequest {
    pub app_id: u32,
    pub published_file_id: u64,
    pub description: String,
    pub language: Option<String>,
    pub change_note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreviewUpdateRequest {
    pub app_id: u32,
    pub published_file_id: u64,
    pub preview_file: String,
    pub change_note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QueryWorkshopItemRequest {
    pub app_id: u32,
    pub published_file_id: u64,
    pub language: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct QueriedWorkshopItem {
    pub app_id: u32,
    pub published_file_id: u64,
    pub title: String,
    pub description: String,
    pub visibility: u8,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SteamClientStatus {
    pub available: bool,
    pub app_id: u32,
    pub steam_id: Option<u64>,
    pub persona_name: Option<String>,
    pub logged_on: Option<bool>,
    pub error: Option<String>,
}

/// Generate a Valve KeyValues .vdf file content for steamcmd workshop_build_item
#[tauri::command]
fn generate_workshop_vdf(item: WorkshopItem) -> Result<String, String> {
    if item.title.trim().is_empty() {
        return Err("Title is required".to_string());
    }
    if item.content_folder.trim().is_empty() {
        return Err("Content folder is required".to_string());
    }
    if item.app_id == 0 {
        return Err("App ID must be greater than 0".to_string());
    }

    let published = item.published_file_id.unwrap_or(0);
    let visibility = item.visibility.min(2);

    let mut vdf = String::new();
    vdf.push_str("\"workshopitem\"\n{\n");

    vdf.push_str(&format!("\t\"appid\"\t\t\t\"{}\"\n", item.app_id));
    vdf.push_str(&format!("\t\"publishedfileid\"\t\"{}\"\n", published));
    vdf.push_str(&format!(
        "\t\"contentfolder\"\t\"{}\"\n",
        escape_vdf_path(&item.content_folder)
    ));

    if let Some(preview) = &item.preview_file {
        if !preview.trim().is_empty() {
            vdf.push_str(&format!(
                "\t\"previewfile\"\t\"{}\"\n",
                escape_vdf_path(preview)
            ));
        }
    }

    vdf.push_str(&format!(
        "\t\"title\"\t\t\t\"{}\"\n",
        escape_vdf_string(&item.title)
    ));
    vdf.push_str(&format!(
        "\t\"description\"\t\t\"{}\"\n",
        escape_vdf_string(&item.description)
    ));
    vdf.push_str(&format!("\t\"visibility\"\t\t\"{}\"\n", visibility));

    if let Some(note) = &item.change_note {
        if !note.trim().is_empty() {
            vdf.push_str(&format!(
                "\t\"changenote\"\t\t\"{}\"\n",
                escape_vdf_string(note)
            ));
        }
    }

    if !item.tags.is_empty() {
        vdf.push_str("\t\"tags\"\n\t{\n");
        for (i, tag) in item.tags.iter().enumerate() {
            if !tag.trim().is_empty() {
                vdf.push_str(&format!(
                    "\t\t\"{}\"\t\t\"{}\"\n",
                    i,
                    escape_vdf_string(tag)
                ));
            }
        }
        vdf.push_str("\t}\n");
    }

    vdf.push_str("}\n");
    Ok(vdf)
}

/// Very simple VDF string escaper (handles quotes and backslashes)
fn escape_vdf_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Normalize path for VDF (Windows prefers \\, but / works cross-platform in most cases)
fn escape_vdf_path(p: &str) -> String {
    // Use forward slashes for maximum compatibility across OSes in steamcmd
    p.replace('\\', "/")
}

/// Write the VDF content to a temporary file and return its path.
/// Useful so the frontend doesn't have to deal with temp dirs.
#[tauri::command]
async fn write_temp_vdf(app: tauri::AppHandle, content: String) -> Result<String, String> {
    let temp_dir = app.path().temp_dir().map_err(|e| e.to_string())?;
    let vdf_path = temp_dir.join(format!(
        "workshop_item_{}.vdf",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));

    std::fs::write(&vdf_path, content).map_err(|e| e.to_string())?;
    Ok(vdf_path.to_string_lossy().to_string())
}

/// Start a workshop upload by spawning steamcmd.
/// Emits events: "workshop-log" (with {line: string, stream: "stdout"|"stderr"})
/// and "workshop-complete" when finished.
#[tauri::command]
async fn start_workshop_upload(
    app: tauri::AppHandle,
    steamcmd_path: String,
    vdf_path: String,
) -> Result<(), String> {
    if !Path::new(&steamcmd_path).exists() {
        return Err(format!("steamcmd not found at: {}", steamcmd_path));
    }
    if !Path::new(&vdf_path).exists() {
        return Err(format!("VDF file not found at: {}", vdf_path));
    }

    let is_windows = cfg!(target_os = "windows");
    let cmd_name = if is_windows && !steamcmd_path.ends_with(".exe") {
        format!("{}.exe", steamcmd_path)
    } else {
        steamcmd_path.clone()
    };

    // Build the command: steamcmd +workshop_build_item "path/to.vdf" +quit
    let args = vec![
        "+workshop_build_item".to_string(),
        vdf_path.clone(),
        "+quit".to_string(),
    ];

    app.emit(
        "workshop-log",
        serde_json::json!({
            "line": format!("> {} {}", cmd_name, args.join(" ")),
            "stream": "info"
        }),
    )
    .ok();

    let shell = app.shell();
    let (mut rx, _child) = shell
        .command(cmd_name)
        .args(args)
        .spawn()
        .map_err(|e| format!("Failed to spawn steamcmd: {}", e))?;

    // Spawn a task to forward all output as events
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let text = String::from_utf8_lossy(&line);
                    for l in text.lines() {
                        if !l.trim().is_empty() {
                            let _ = app_clone.emit(
                                "workshop-log",
                                serde_json::json!({
                                    "line": l,
                                    "stream": "stdout"
                                }),
                            );
                        }
                    }
                }
                CommandEvent::Stderr(line) => {
                    let text = String::from_utf8_lossy(&line);
                    for l in text.lines() {
                        if !l.trim().is_empty() {
                            let _ = app_clone.emit(
                                "workshop-log",
                                serde_json::json!({
                                    "line": l,
                                    "stream": "stderr"
                                }),
                            );
                        }
                    }
                }
                CommandEvent::Error(err) => {
                    let _ = app_clone.emit(
                        "workshop-log",
                        serde_json::json!({
                            "line": format!("ERROR: {}", err),
                            "stream": "stderr"
                        }),
                    );
                }
                CommandEvent::Terminated(payload) => {
                    let success = payload.code.map_or(false, |c| c == 0);
                    let _ = app_clone.emit(
                        "workshop-complete",
                        serde_json::json!({
                            "success": success,
                            "code": payload.code
                        }),
                    );
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(())
}

/// Simple helper to check if a path looks like a valid steamcmd binary
#[tauri::command]
fn is_valid_steamcmd(path: String) -> bool {
    let p = Path::new(&path);
    if !p.exists() {
        return false;
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.contains("steamcmd")
}

/// Check if we can initialize Steamworks for the given AppID.
/// This succeeds when the Steam client is running and the user is logged in.
#[cfg(feature = "steamworks-sdk")]
#[tauri::command]
fn check_steam_client_status(app_id: u32) -> SteamClientStatus {
    probe_steam_client_status_in_child(app_id)
}

#[cfg(feature = "steamworks-sdk")]
fn probe_steam_client_status_in_child(app_id: u32) -> SteamClientStatus {
    if app_id == 0 {
        return SteamClientStatus {
            available: false,
            app_id,
            steam_id: None,
            persona_name: None,
            logged_on: None,
            error: Some("App ID must be greater than 0".to_string()),
        };
    }

    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            return SteamClientStatus {
                available: false,
                app_id,
                steam_id: None,
                persona_name: None,
                logged_on: None,
                error: Some(format!("Could not locate executable for status probe: {}", e)),
            }
        }
    };

    let mut child = match Command::new(exe)
        .arg(STEAM_STATUS_PROBE_ARG)
        .arg(app_id.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return SteamClientStatus {
                available: false,
                app_id,
                steam_id: None,
                persona_name: None,
                logged_on: None,
                error: Some(format!("Could not spawn status probe process: {}", e)),
            }
        }
    };

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if started.elapsed() > STEAM_STATUS_PROBE_TIMEOUT {
                    let _ = child.kill();
                    break Err("Status probe timed out".to_string());
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => break Err(format!("Status probe wait failed: {}", e)),
        }
    };

    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }

    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }

    match status {
        Ok(exit) => {
            if !exit.success() {
                return SteamClientStatus {
                    available: false,
                    app_id,
                    steam_id: None,
                    persona_name: None,
                    logged_on: None,
                    error: Some(if stderr.trim().is_empty() {
                        format!("Status probe exited with code {:?}", exit.code())
                    } else {
                        format!("Status probe exited with code {:?}: {}", exit.code(), stderr.trim())
                    }),
                };
            }

            match serde_json::from_str::<SteamClientStatus>(stdout.trim()) {
                Ok(mut parsed) => {
                    if parsed.app_id == 0 {
                        parsed.app_id = app_id;
                    }
                    parsed
                }
                Err(e) => SteamClientStatus {
                    available: false,
                    app_id,
                    steam_id: None,
                    persona_name: None,
                    logged_on: None,
                    error: Some(format!(
                        "Status probe returned invalid JSON: {}{}",
                        e,
                        if stdout.trim().is_empty() {
                            "".to_string()
                        } else {
                            format!(". Raw output: {}", stdout.trim())
                        }
                    )),
                },
            }
        }
        Err(err) => SteamClientStatus {
            available: false,
            app_id,
            steam_id: None,
            persona_name: None,
            logged_on: None,
            error: Some(err),
        },
    }
}

#[cfg(feature = "steamworks-sdk")]
pub fn try_handle_steam_status_probe_process() -> bool {
    let mut args = std::env::args();
    let _ = args.next();
    if args.next().as_deref() != Some(STEAM_STATUS_PROBE_ARG) {
        return false;
    }

    let app_id = args
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0);

    let status = std::panic::catch_unwind(|| steamworks::steam_client_status(app_id)).unwrap_or(
        SteamClientStatus {
            available: false,
            app_id,
            steam_id: None,
            persona_name: None,
            logged_on: None,
            error: Some("Steamworks probe panicked in child process".to_string()),
        },
    );

    match serde_json::to_string(&status) {
        Ok(json) => {
            println!("{}", json);
        }
        Err(e) => {
            eprintln!("Could not serialize status probe result: {}", e);
        }
    }

    true
}

#[cfg(not(feature = "steamworks-sdk"))]
pub fn try_handle_steam_status_probe_process() -> bool {
    false
}

/// Upload / update a workshop item using the Steamworks SDK.
/// The user must have the Steam client running and logged in.
#[cfg(feature = "steamworks-sdk")]
#[tauri::command]
async fn upload_via_steamworks(
    app: tauri::AppHandle,
    item: WorkshopItem,
) -> Result<UploadResult, String> {
    // Run the blocking Steamworks logic in a blocking task
    tauri::async_runtime::spawn_blocking(move || steamworks::upload_item_via_steamworks(app, item))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Update only the Workshop description (optionally with a specific language) via Steamworks SDK.
#[cfg(feature = "steamworks-sdk")]
#[tauri::command]
async fn update_description_via_steamworks(
    app: tauri::AppHandle,
    req: DescriptionUpdateRequest,
) -> Result<UploadResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        steamworks::update_item_description_via_steamworks(app, req)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Update only the Workshop preview image via Steamworks SDK.
#[cfg(feature = "steamworks-sdk")]
#[tauri::command]
async fn update_preview_via_steamworks(
    app: tauri::AppHandle,
    req: PreviewUpdateRequest,
) -> Result<UploadResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        steamworks::update_item_preview_via_steamworks(app, req)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Query a Workshop item by Published File ID and return metadata for form backfill.
#[cfg(feature = "steamworks-sdk")]
#[tauri::command]
async fn query_workshop_item_by_id(
    app: tauri::AppHandle,
    req: QueryWorkshopItemRequest,
) -> Result<QueriedWorkshopItem, String> {
    tauri::async_runtime::spawn_blocking(move || steamworks::query_workshop_item_by_id(app, req))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            greet,
            generate_workshop_vdf,
            write_temp_vdf,
            start_workshop_upload,
            is_valid_steamcmd,
            #[cfg(feature = "steamworks-sdk")]
            check_steam_client_status,
            #[cfg(feature = "steamworks-sdk")]
            upload_via_steamworks,
            #[cfg(feature = "steamworks-sdk")]
            update_description_via_steamworks,
            #[cfg(feature = "steamworks-sdk")]
            update_preview_via_steamworks,
            #[cfg(feature = "steamworks-sdk")]
            query_workshop_item_by_id,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

use std::{
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use steamworks::{AppId, Client, FileType, PublishedFileId, PublishedFileVisibility, UpdateStatus};
use tauri::Emitter;

use crate::{SteamClientStatus, UploadResult, WorkshopItem};

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
const CALLBACK_TICK: Duration = Duration::from_millis(50);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// Attempts to initialize the Steamworks client for the given AppID.
/// This succeeds when the Steam client is running and the user is logged in.
pub fn try_init_steamworks(app_id: u32) -> Result<(Client, steamworks::SingleClient), String> {
    Client::init_app(app_id).map_err(|e| {
        format!(
            "Could not connect to the Steam client (AppID {}).\n\n\
             Please ensure:\n\
             - Steam is running and you are logged in\n\
             - You have workshop permissions for this game\n\n\
             Error: {}",
            app_id, e
        )
    })
}

/// Returns Steam client availability plus current user details when Steamworks initializes.
pub fn steam_client_status(app_id: u32) -> SteamClientStatus {
    match try_init_steamworks(app_id) {
        Ok((client, _single)) => {
            let user = client.user();
            let friends = client.friends();
            SteamClientStatus {
                available: true,
                app_id,
                steam_id: Some(user.steam_id().raw()),
                persona_name: Some(friends.name()),
                logged_on: Some(user.logged_on()),
                error: None,
            }
        }
        Err(error) => SteamClientStatus {
            available: false,
            app_id,
            steam_id: None,
            persona_name: None,
            logged_on: None,
            error: Some(error),
        },
    }
}

/// Uploads or updates a Workshop item using ISteamUGC.
pub fn upload_item_via_steamworks(
    app: tauri::AppHandle,
    item: WorkshopItem,
) -> Result<UploadResult, String> {
    match upload_item_via_steamworks_inner(app.clone(), item) {
        Ok(result) => Ok(result),
        Err(err) => {
            let _ = app.emit(
                "workshop-complete",
                serde_json::json!({
                    "success": false,
                    "code": null,
                    "method": "sdk",
                    "error": err
                }),
            );
            Err(err)
        }
    }
}

fn upload_item_via_steamworks_inner(
    app: tauri::AppHandle,
    mut item: WorkshopItem,
) -> Result<UploadResult, String> {
    validate_item(&mut item)?;
    emit_log(&app, "Initializing Steamworks SDK...", "info");

    let (client, single) = try_init_steamworks(item.app_id)?;
    let ugc = client.ugc();
    let app_id = AppId(item.app_id);

    let mut needs_legal_agreement = false;
    let published_file_id = match item.published_file_id.filter(|id| *id > 0) {
        Some(id) => {
            emit_log(&app, &format!("Updating Workshop item {}", id), "info");
            PublishedFileId(id)
        }
        None => {
            emit_log(&app, "Creating new Workshop item...", "info");
            let (id, needs_agreement) = create_item(&single, &ugc, app_id)?;
            needs_legal_agreement = needs_agreement;
            id
        }
    };

    emit_log(&app, "Preparing Workshop item update...", "info");
    let mut update = ugc
        .start_item_update(app_id, published_file_id)
        .title(&item.title)
        .description(&item.description)
        .content_path(Path::new(&item.content_folder))
        .visibility(map_visibility(item.visibility));

    if let Some(preview) = &item.preview_file {
        if !preview.trim().is_empty() {
            update = update.preview_path(Path::new(preview));
        }
    }

    if !item.tags.is_empty() {
        update = update.tags(item.tags.clone(), false);
    }

    emit_log(&app, "Submitting Workshop item update...", "info");
    let (tx, rx) = mpsc::channel();
    let watch = update.submit(item.change_note.as_deref(), move |result| {
        let _ = tx.send(result);
    });

    let mut last_progress = Instant::now() - PROGRESS_INTERVAL;
    let result = wait_for_callback(&single, &rx, || {
        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            last_progress = Instant::now();
            let (status, current, total) = watch.progress();
            emit_progress(&app, status, current, total);
        }
    })?;

    let (final_id, submit_needs_legal_agreement) =
        result.map_err(|e| format!("Steamworks upload failed: {}", e))?;
    needs_legal_agreement = needs_legal_agreement || submit_needs_legal_agreement;

    if needs_legal_agreement {
        emit_log(
            &app,
            "Upload completed, but Steam requires accepting the Workshop legal agreement.",
            "info",
        );
    }

    emit_log(
        &app,
        &format!(
            "Steamworks upload completed. PublishedFileID: {}",
            final_id.0
        ),
        "info",
    );
    let _ = app.emit(
        "workshop-complete",
        serde_json::json!({
            "success": true,
            "code": 0,
            "method": "sdk",
            "publishedFileId": final_id.0,
            "needsLegalAgreement": needs_legal_agreement
        }),
    );

    Ok(UploadResult {
        published_file_id: final_id.0,
        needs_legal_agreement,
        method: "sdk".to_string(),
    })
}

fn create_item(
    single: &steamworks::SingleClient,
    ugc: &steamworks::UGC<steamworks::ClientManager>,
    app_id: AppId,
) -> Result<(PublishedFileId, bool), String> {
    let (tx, rx) = mpsc::channel();
    ugc.create_item(app_id, FileType::Community, move |result| {
        let _ = tx.send(result);
    });

    let result = wait_for_callback(single, &rx, || {})?;
    result.map_err(|e| format!("Could not create Workshop item: {}", e))
}

fn wait_for_callback<T, F>(
    single: &steamworks::SingleClient,
    rx: &mpsc::Receiver<T>,
    mut on_tick: F,
) -> Result<T, String>
where
    F: FnMut(),
{
    let started = Instant::now();
    loop {
        single.run_callbacks();

        match rx.try_recv() {
            Ok(result) => return Ok(result),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("Steamworks callback channel disconnected".to_string());
            }
        }

        if started.elapsed() > CALLBACK_TIMEOUT {
            return Err("Timed out waiting for Steamworks callback".to_string());
        }

        on_tick();
        thread::sleep(CALLBACK_TICK);
    }
}

fn validate_item(item: &mut WorkshopItem) -> Result<(), String> {
    if item.app_id == 0 {
        return Err("App ID must be greater than 0".to_string());
    }

    item.title = item.title.trim().to_string();
    if item.title.is_empty() {
        return Err("Title is required".to_string());
    }

    reject_nul("title", &item.title)?;
    reject_nul("description", &item.description)?;
    if let Some(note) = &item.change_note {
        reject_nul("change note", note)?;
    }

    let content_path = Path::new(item.content_folder.trim());
    if !content_path.exists() {
        return Err(format!("Content folder not found: {}", item.content_folder));
    }
    if !content_path.is_dir() {
        return Err(format!(
            "Content path must be a folder: {}",
            item.content_folder
        ));
    }
    item.content_folder = content_path
        .canonicalize()
        .map_err(|e| format!("Could not resolve content folder: {}", e))?
        .to_string_lossy()
        .to_string();
    reject_nul("content folder", &item.content_folder)?;

    if let Some(preview) = &item.preview_file {
        if preview.trim().is_empty() {
            item.preview_file = None;
        } else {
            let preview_path = Path::new(preview.trim());
            if !preview_path.exists() {
                return Err(format!("Preview file not found: {}", preview));
            }
            if !preview_path.is_file() {
                return Err(format!("Preview path must be a file: {}", preview));
            }
            let resolved = preview_path
                .canonicalize()
                .map_err(|e| format!("Could not resolve preview file: {}", e))?
                .to_string_lossy()
                .to_string();
            reject_nul("preview file", &resolved)?;
            item.preview_file = Some(resolved);
        }
    }

    item.tags = item
        .tags
        .iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    for tag in &item.tags {
        reject_nul("tag", tag)?;
    }

    Ok(())
}

fn reject_nul(field: &str, value: &str) -> Result<(), String> {
    if value.contains('\0') {
        Err(format!("{} cannot contain NUL bytes", field))
    } else {
        Ok(())
    }
}

fn map_visibility(visibility: u8) -> PublishedFileVisibility {
    match visibility.min(2) {
        0 => PublishedFileVisibility::Public,
        1 => PublishedFileVisibility::FriendsOnly,
        _ => PublishedFileVisibility::Private,
    }
}

fn emit_progress(app: &tauri::AppHandle, status: UpdateStatus, current: u64, total: u64) {
    let status_text = match status {
        UpdateStatus::Invalid => "Invalid",
        UpdateStatus::PreparingConfig => "Preparing config",
        UpdateStatus::PreparingContent => "Preparing content",
        UpdateStatus::UploadingContent => "Uploading content",
        UpdateStatus::UploadingPreviewFile => "Uploading preview",
        UpdateStatus::CommittingChanges => "Committing changes",
    };

    let line = if total > 0 {
        let percent = current.saturating_mul(100) / total;
        format!(
            "{}: {} / {} bytes ({}%)",
            status_text, current, total, percent
        )
    } else {
        status_text.to_string()
    };

    emit_log(app, &line, "stdout");
}

fn emit_log(app: &tauri::AppHandle, line: &str, stream: &str) {
    let _ = app.emit(
        "workshop-log",
        serde_json::json!({
            "line": line,
            "stream": stream
        }),
    );
}

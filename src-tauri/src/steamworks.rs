use std::{
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use steamworks::{AppId, Client, FileType, PublishedFileId, PublishedFileVisibility, UpdateStatus};
use tauri::Emitter;

use crate::{
    DescriptionUpdateRequest, PreviewUpdateRequest, QueriedWorkshopItem, QueryWorkshopItemRequest,
    SteamClientStatus, UploadResult, WorkshopItem,
};

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(300);
const CALLBACK_TICK: Duration = Duration::from_millis(50);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// Attempts to initialize the Steamworks client for the given AppID.
/// This succeeds when the Steam client is running and the user is logged in.
pub fn try_init_steamworks(app_id: u32) -> Result<Client, String> {
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
        Ok(client) => {
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

/// Updates only the description of an existing Workshop item.
pub fn update_item_description_via_steamworks(
    app: tauri::AppHandle,
    req: DescriptionUpdateRequest,
) -> Result<UploadResult, String> {
    match update_item_description_via_steamworks_inner(app.clone(), req) {
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

/// Updates only the preview image of an existing Workshop item.
pub fn update_item_preview_via_steamworks(
    app: tauri::AppHandle,
    req: PreviewUpdateRequest,
) -> Result<UploadResult, String> {
    match update_item_preview_via_steamworks_inner(app.clone(), req) {
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

/// Queries one Workshop item by Published File ID for metadata backfill.
pub fn query_workshop_item_by_id(
    app: tauri::AppHandle,
    req: QueryWorkshopItemRequest,
) -> Result<QueriedWorkshopItem, String> {
    let mut req = req;
    validate_query_request(&mut req)?;

    emit_log(
        &app,
        &format!("Querying Workshop item {}...", req.published_file_id),
        "info",
    );

    // Use Steam Web API for item metadata query to avoid SDK query edge-case crashes.
    let endpoint = "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";
    let body = format!("itemcount=1&publishedfileids[0]={}", req.published_file_id);
    let response = ureq::post(endpoint)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
        .map_err(|e| format!("Workshop query request failed: {}", e))?;

    let response_value: serde_json::Value = response
        .into_json()
        .map_err(|e| format!("Could not parse Workshop query response: {}", e))?;

    let details = response_value
        .get("response")
        .and_then(|v| v.get("publishedfiledetails"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .ok_or_else(|| "Malformed Workshop query response from Steam Web API.".to_string())?;

    let item_result = details.get("result").and_then(|v| v.as_u64()).unwrap_or(0);
    if item_result != 1 {
        return Err(format!(
            "Workshop item query failed for {} (result code {}).",
            req.published_file_id, item_result
        ));
    }

    let resolved_app_id = details
        .get("consumer_app_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(req.app_id);

    let title = details
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let description = details
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let visibility = match details
        .get("visibility")
        .and_then(|v| v.as_u64())
        .unwrap_or(2)
        .min(3)
    {
        0 => 0,
        1 => 1,
        _ => 2,
    };

    let tags = details
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    if let Some(tag_text) = entry.as_str() {
                        Some(tag_text.to_string())
                    } else {
                        entry
                            .get("tag")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    }
                })
                .map(|tag| tag.trim().to_string())
                .filter(|tag| !tag.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let result = QueriedWorkshopItem {
        app_id: resolved_app_id,
        published_file_id: req.published_file_id,
        title,
        description,
        visibility,
        tags,
    };

    emit_log(
        &app,
        &format!(
            "Workshop item {} queried successfully.",
            result.published_file_id
        ),
        "info",
    );

    Ok(result)
}

fn upload_item_via_steamworks_inner(
    app: tauri::AppHandle,
    mut item: WorkshopItem,
) -> Result<UploadResult, String> {
    validate_item(&mut item)?;
    emit_log(&app, "Initializing Steamworks SDK...", "info");

    let client = try_init_steamworks(item.app_id)?;
    let ugc = client.ugc();
    let app_id = AppId(item.app_id);

    let mut needs_legal_agreement = false;
    let is_update = item.published_file_id.filter(|id| *id > 0).is_some();
    let published_file_id = match item.published_file_id.filter(|id| *id > 0) {
        Some(id) => {
            emit_log(&app, &format!("Updating Workshop item {}", id), "info");
            // Fail early with a clear message if the ID is dead / deleted / wrong app.
            if let Err(msg) = verify_published_file_exists(id) {
                emit_log(&app, &msg, "stderr");
                return Err(msg);
            }
            PublishedFileId(id)
        }
        None => {
            emit_log(&app, "Creating new Workshop item...", "info");
            let (id, needs_agreement) = create_item(&client, &ugc, app_id)?;
            needs_legal_agreement = needs_agreement;
            id
        }
    };

    emit_log(
        &app,
        &format!(
            "Preparing Workshop item update...\n  content: {}\n  preview: {}",
            item.content_folder,
            item.preview_file
                .as_deref()
                .filter(|p| !p.trim().is_empty())
                .unwrap_or("(none)")
        ),
        "info",
    );

    // Re-check paths right before hand-off to Steam (temp packages can disappear).
    if !Path::new(&item.content_folder).is_dir() {
        return Err(format!(
            "Content folder disappeared before upload: {}",
            item.content_folder
        ));
    }
    if let Some(preview) = &item.preview_file {
        if !preview.trim().is_empty() && !Path::new(preview).is_file() {
            return Err(format!(
                "Preview file disappeared before upload: {}",
                preview
            ));
        }
    }

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
    let (final_id, submit_needs_legal_agreement) =
        submit_update_and_wait(&client, &app, update, item.change_note.as_deref()).map_err(
            |err| {
                if is_update && err.to_ascii_lowercase().contains("file was not found") {
                    format!(
                        "{err}\n\n\
                         This usually means Published File ID {} is invalid or was deleted on Steam.\n\
                         Fix: clear About/PublishedFileId.txt (and the Published File ID field), \
                         then upload again as a NEW item.",
                        published_file_id.0
                    )
                } else if err.to_ascii_lowercase().contains("file was not found") {
                    format!(
                        "{err}\n\n\
                         Steam could not read the content folder or preview image.\n\
                         content: {}\n\
                         preview: {}\n\
                         Try regenerating the temp package, or use the original mod folder.",
                        item.content_folder,
                        item.preview_file.as_deref().unwrap_or("(none)")
                    )
                } else {
                    err
                }
            },
        )?;
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

fn update_item_description_via_steamworks_inner(
    app: tauri::AppHandle,
    mut req: DescriptionUpdateRequest,
) -> Result<UploadResult, String> {
    validate_description_request(&mut req)?;

    emit_log(&app, "Initializing Steamworks SDK...", "info");
    let client = try_init_steamworks(req.app_id)?;
    let ugc = client.ugc();
    let app_id = AppId(req.app_id);
    let published_file_id = PublishedFileId(req.published_file_id);

    emit_log(
        &app,
        &format!(
            "Updating description for Workshop item {}{}",
            req.published_file_id,
            req.language
                .as_ref()
                .map(|lang| format!(" (language: {})", lang))
                .unwrap_or_default()
        ),
        "info",
    );

    let mut update = ugc
        .start_item_update(app_id, published_file_id)
        .description(&req.description);

    if let Some(language) = &req.language {
        update = update.language(language);
    }

    emit_log(&app, "Submitting description update...", "info");
    let (final_id, needs_legal_agreement) =
        submit_update_and_wait(&client, &app, update, req.change_note.as_deref())?;

    if needs_legal_agreement {
        emit_log(
            &app,
            "Description update completed, but Steam requires accepting the Workshop legal agreement.",
            "info",
        );
    }

    emit_log(
        &app,
        &format!(
            "Description update completed. PublishedFileID: {}",
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

fn update_item_preview_via_steamworks_inner(
    app: tauri::AppHandle,
    mut req: PreviewUpdateRequest,
) -> Result<UploadResult, String> {
    validate_preview_request(&mut req)?;

    emit_log(&app, "Initializing Steamworks SDK...", "info");
    let client = try_init_steamworks(req.app_id)?;
    let ugc = client.ugc();
    let app_id = AppId(req.app_id);
    let published_file_id = PublishedFileId(req.published_file_id);

    emit_log(
        &app,
        &format!(
            "Updating preview image for Workshop item {}",
            req.published_file_id
        ),
        "info",
    );

    let update = ugc
        .start_item_update(app_id, published_file_id)
        .preview_path(Path::new(&req.preview_file));

    emit_log(&app, "Submitting preview image update...", "info");
    let (final_id, needs_legal_agreement) =
        submit_update_and_wait(&client, &app, update, req.change_note.as_deref())?;

    if needs_legal_agreement {
        emit_log(
            &app,
            "Preview image update completed, but Steam requires accepting the Workshop legal agreement.",
            "info",
        );
    }

    emit_log(
        &app,
        &format!(
            "Preview image update completed. PublishedFileID: {}",
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

/// Soft-check via Steam Web API whether a published file still exists.
/// Result code 1 = OK; 9 = FileNotFound (deleted / never existed / private+inaccessible).
fn verify_published_file_exists(published_file_id: u64) -> Result<(), String> {
    let endpoint = "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";
    let body = format!("itemcount=1&publishedfileids[0]={}", published_file_id);
    let response = match ureq::post(endpoint)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
    {
        Ok(r) => r,
        Err(e) => {
            // Network failure should not block upload — Steam client may still succeed.
            eprintln!("PublishedFileId pre-check skipped (network): {}", e);
            return Ok(());
        }
    };

    let value: serde_json::Value = match response.into_json() {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let details = value
        .get("response")
        .and_then(|v| v.get("publishedfiledetails"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first());

    let Some(details) = details else {
        return Ok(());
    };

    let result_code = details.get("result").and_then(|v| v.as_u64()).unwrap_or(0);
    if result_code == 1 {
        return Ok(());
    }

    // 9 = FileNotFound
    if result_code == 9 {
        return Err(format!(
            "Published File ID {} was not found on Steam (deleted or never published successfully).\n\n\
             Fix:\n\
             1. Clear the Published File ID field in the app\n\
             2. Delete About/PublishedFileId.txt in your mod folder (if present)\n\
             3. Upload again as a NEW item\n\n\
             Steam API result code: {}",
            published_file_id, result_code
        ));
    }

    // Other codes — warn but allow Steam client path to proceed.
    Ok(())
}

fn submit_update_and_wait(
    client: &Client,
    app: &tauri::AppHandle,
    update: steamworks::UpdateHandle,
    change_note: Option<&str>,
) -> Result<(PublishedFileId, bool), String> {
    let (tx, rx) = mpsc::channel();
    let watch = update.submit(change_note, move |result| {
        let _ = tx.send(result);
    });

    let mut last_progress = Instant::now() - PROGRESS_INTERVAL;
    let result = wait_for_callback(client, &rx, || {
        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            last_progress = Instant::now();
            let (status, current, total) = watch.progress();
            emit_progress(app, status, current, total);
        }
    })?;

    result.map_err(|e| format!("Steamworks upload failed: {}", e))
}

fn create_item(
    client: &Client,
    ugc: &steamworks::UGC,
    app_id: AppId,
) -> Result<(PublishedFileId, bool), String> {
    let (tx, rx) = mpsc::channel();
    ugc.create_item(app_id, FileType::Community, move |result| {
        let _ = tx.send(result);
    });

    let result = wait_for_callback(client, &rx, || {})?;
    result.map_err(|e| format!("Could not create Workshop item: {}", e))
}

fn wait_for_callback<T, F>(
    client: &Client,
    rx: &mpsc::Receiver<T>,
    mut on_tick: F,
) -> Result<T, String>
where
    F: FnMut(),
{
    let started = Instant::now();
    loop {
        client.run_callbacks();

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

fn validate_description_request(req: &mut DescriptionUpdateRequest) -> Result<(), String> {
    if req.app_id == 0 {
        return Err("App ID must be greater than 0".to_string());
    }
    if req.published_file_id == 0 {
        return Err("Published File ID must be greater than 0".to_string());
    }

    req.description = req.description.trim().to_string();
    reject_nul("description", &req.description)?;

    if let Some(language) = &req.language {
        let normalized = language.trim().to_string();
        if normalized.is_empty() {
            req.language = None;
        } else {
            reject_nul("language", &normalized)?;
            req.language = Some(normalized);
        }
    }

    if let Some(note) = &req.change_note {
        let normalized = note.trim().to_string();
        if normalized.is_empty() {
            req.change_note = None;
        } else {
            reject_nul("change note", &normalized)?;
            req.change_note = Some(normalized);
        }
    }

    Ok(())
}

fn validate_preview_request(req: &mut PreviewUpdateRequest) -> Result<(), String> {
    if req.app_id == 0 {
        return Err("App ID must be greater than 0".to_string());
    }
    if req.published_file_id == 0 {
        return Err("Published File ID must be greater than 0".to_string());
    }

    let preview_path = Path::new(req.preview_file.trim());
    if !preview_path.exists() {
        return Err(format!("Preview file not found: {}", req.preview_file));
    }
    if !preview_path.is_file() {
        return Err(format!("Preview path must be a file: {}", req.preview_file));
    }
    req.preview_file = preview_path
        .canonicalize()
        .map_err(|e| format!("Could not resolve preview file: {}", e))?
        .to_string_lossy()
        .to_string();
    reject_nul("preview file", &req.preview_file)?;

    if let Some(note) = &req.change_note {
        let normalized = note.trim().to_string();
        if normalized.is_empty() {
            req.change_note = None;
        } else {
            reject_nul("change note", &normalized)?;
            req.change_note = Some(normalized);
        }
    }

    Ok(())
}

fn validate_query_request(req: &mut QueryWorkshopItemRequest) -> Result<(), String> {
    if req.app_id == 0 {
        return Err("App ID must be greater than 0".to_string());
    }
    if req.published_file_id == 0 {
        return Err("Published File ID must be greater than 0".to_string());
    }

    if let Some(language) = &req.language {
        let normalized = language.trim().to_string();
        if normalized.is_empty() {
            req.language = None;
        } else {
            reject_nul("language", &normalized)?;
            req.language = Some(normalized);
        }
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

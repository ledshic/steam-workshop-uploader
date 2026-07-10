//! Mount & Blade II: Bannerlord module detection and clean packaging.
//!
//! Rules distilled from workspace modules (`Bannerlord.*`, `MnB_*`, `bannerlord/*`):
//!
//! ## Ship-ready module root (upload content)
//! ```text
//! ModuleId/
//!   SubModule.xml                 # required
//!   bin/Win64_Shipping_Client/    # compiled DLLs
//!   ModuleData/                   # optional XML / languages
//!   GUI/ AssetPackages/ ...       # optional game assets
//! ```
//!
//! ## Common repo layouts (not all upload-ready as-is)
//! - `out/ModuleId/` — preferred build output (ready to upload)
//! - `_Module/` — install template / partial module folder
//! - `_Workshop/` — Steam preview images + WorkshopUpdate.xml (not module content)
//! - `dev/`, `src/`, `.git/` — development only
//!
//! ## SubModule.xml fields
//! - `<Name value="..."/>`, `<Id value="..."/>`, `<Version value="vX.Y.Z"/>`
//! - `<SingleplayerModule/>` / `<MultiplayerModule/>` / `<ModuleCategory/>`
//! - `<DependedModules><DependedModule Id="..."/></DependedModules>`
//!
//! ## Steam
//! - AppID **261550**
//! - Tags often: Utility / UI / Singleplayer / Compatible Version
//! - PublishedFileId from `WorkshopUpdate.xml` `<ItemId Value="..."/>`

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Mount & Blade II: Bannerlord Steam AppID.
pub const BANNERLORD_APP_ID: u32 = 261550;

const PACKAGE_EXCLUDE_DIRS: &[&str] = &[
    ".git",
    ".github",
    ".vs",
    ".vscode",
    ".idea",
    "Source",
    "src",
    "dev",
    "obj",
    "node_modules",
    "_Workshop",
    "_workshop",
    "tests",
    "docs",
    "build",
    "properties",
    "Properties",
];

const PACKAGE_EXCLUDE_FILE_NAMES: &[&str] = &[
    ".DS_Store",
    ".gitignore",
    ".gitattributes",
    "Directory.Build.props",
    "Directory.Build.targets",
    "Thumbs.db",
    "desktop.ini",
    "packages.config",
    "steam_appid.txt", // game AppID helper, not module content
];

const PACKAGE_EXCLUDE_EXTENSIONS: &[&str] = &[
    "sln",
    "csproj",
    "user",
    "suo",
    "pdb",
    "md",
    "code-workspace",
    "cs",
    "vdf",
    "pdn", // Paint.NET source
    "xlsx",
    "bat",
    "ps1",
    "env",
    "example",
];

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BannerlordModInfo {
    pub mod_root: String,
    pub content_folder: String,
    pub submodule_xml_path: String,
    pub name: String,
    pub module_id: String,
    pub version: Option<String>,
    pub description: String,
    pub singleplayer: bool,
    pub multiplayer: bool,
    pub depended_modules: Vec<String>,
    pub preview_file: Option<String>,
    pub published_file_id: Option<u64>,
    pub published_file_id_path: Option<String>,
    pub tags: Vec<String>,
    pub warnings: Vec<String>,
    pub detected_files: Vec<String>,
    pub is_packaged: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreparePackageRequest {
    pub mod_root: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WritePublishedFileIdRequest {
    pub mod_root: String,
    pub published_file_id: u64,
}

pub fn detect_bannerlord_mod(path: String) -> Result<BannerlordModInfo, String> {
    let input = PathBuf::from(path.trim());
    if !input.exists() {
        return Err(format!("Path not found: {}", input.display()));
    }

    let mod_root = resolve_module_root(&input)?;
    let submodule_xml = mod_root.join("SubModule.xml");
    if !submodule_xml.is_file() {
        return Err(format!(
            "Not a Bannerlord module: missing SubModule.xml under {}",
            mod_root.display()
        ));
    }

    let raw = fs::read_to_string(&submodule_xml)
        .map_err(|e| format!("Could not read SubModule.xml: {}", e))?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);

    let name = attr_value(raw, "Name")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "SubModule.xml is missing <Name value=\"...\"/>".to_string())?;
    let module_id = attr_value(raw, "Id")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| name.clone());
    let version = attr_value(raw, "Version");

    let singleplayer = bool_attr(raw, "SingleplayerModule").unwrap_or_else(|| {
        attr_value(raw, "ModuleCategory")
            .map(|c| c.eq_ignore_ascii_case("Singleplayer"))
            .unwrap_or(true)
    });
    let multiplayer = bool_attr(raw, "MultiplayerModule").unwrap_or(false);

    let depended_modules = depended_module_ids(raw);
    let mut warnings = Vec::new();
    let mut detected_files = vec![submodule_xml.display().to_string()];

    // Description: README near module or parent repo
    let description = find_readme_description(&mod_root).unwrap_or_else(|| {
        build_fallback_description(&name, &module_id, version.as_deref(), &depended_modules)
    });

    // DLLs
    let dll_dir = mod_root.join("bin").join("Win64_Shipping_Client");
    if dll_dir.is_dir() {
        let dlls: Vec<_> = fs::read_dir(&dll_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x.eq_ignore_ascii_case("dll"))
                    .unwrap_or(false)
            })
            .collect();
        if dlls.is_empty() {
            warnings.push(
                "bin/Win64_Shipping_Client has no .dll — module may be incomplete for upload."
                    .to_string(),
            );
        } else {
            for d in dlls {
                detected_files.push(d.path().display().to_string());
            }
        }
    } else {
        warnings.push(
            "Missing bin/Win64_Shipping_Client — package a built module (e.g. out/ModuleId/) before upload."
                .to_string(),
        );
    }

    let (preview_file, preview_warnings) = find_preview(&mod_root);
    warnings.extend(preview_warnings);
    if let Some(ref p) = preview_file {
        detected_files.push(p.clone());
    } else {
        warnings.push(
            "No preview image found (Image.png / Preview.png / _Workshop/*.png).".to_string(),
        );
    }

    let (published_file_id, published_file_id_path, id_warnings) =
        find_published_file_id(&mod_root);
    warnings.extend(id_warnings);
    if let Some(ref p) = published_file_id_path {
        if Path::new(p).is_file() {
            detected_files.push(p.clone());
        }
    }

    // Junk flags for packaging guidance
    if let Ok(entries) = fs::read_dir(&mod_root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                warnings.push(format!(
                    "Found symlink '{}' — clean packaging will skip it.",
                    name
                ));
            }
        }
    }
    // Parent repo junk when module is nested
    if let Some(parent) = mod_root.parent() {
        for junk in ["dev", "src", ".git", "obj"] {
            if parent.join(junk).is_dir() {
                warnings.push(format!(
                    "Parent repo has '{}' — clean packaging uploads only the module folder.",
                    junk
                ));
            }
        }
    }

    let tags = build_tags(singleplayer, multiplayer, version.as_deref());

    let mod_root_str = canonicalize_path(&mod_root)?;
    Ok(BannerlordModInfo {
        content_folder: mod_root_str.clone(),
        mod_root: mod_root_str,
        submodule_xml_path: canonicalize_path(&submodule_xml)?,
        name,
        module_id,
        version,
        description,
        singleplayer,
        multiplayer,
        depended_modules,
        preview_file: preview_file.and_then(|p| canonicalize_path(Path::new(&p)).ok()),
        published_file_id,
        published_file_id_path,
        tags,
        warnings,
        detected_files,
        is_packaged: false,
    })
}

pub fn prepare_bannerlord_package(req: PreparePackageRequest) -> Result<BannerlordModInfo, String> {
    let mut info = detect_bannerlord_mod(req.mod_root)?;
    let mod_root = PathBuf::from(&info.mod_root);

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let package_name = sanitize_folder_name(&info.module_id);
    let package_root = std::env::temp_dir().join(format!(
        "bannerlord_workshop_{}_{}",
        package_name, stamp
    ));

    if package_root.exists() {
        fs::remove_dir_all(&package_root)
            .map_err(|e| format!("Could not clear previous package dir: {}", e))?;
    }
    fs::create_dir_all(&package_root)
        .map_err(|e| format!("Could not create package dir: {}", e))?;

    copy_filtered(&mod_root, &package_root, &mod_root)
        .map_err(|e| format!("Failed while packaging module: {}", e))?;

    // Ensure SubModule.xml made it
    if !package_root.join("SubModule.xml").is_file() {
        return Err("Package is missing SubModule.xml after copy".to_string());
    }

    let mut packaged = detect_bannerlord_mod(package_root.to_string_lossy().to_string())?;
    packaged.mod_root = info.mod_root.clone();
    packaged.published_file_id = info.published_file_id.or(packaged.published_file_id);
    packaged.published_file_id_path = info.published_file_id_path.clone();
    // Prefer original preview if package has none (preview often lives in _Workshop)
    if packaged.preview_file.is_none() {
        packaged.preview_file = info.preview_file.clone();
    }
    packaged.is_packaged = true;
    packaged.warnings.append(&mut info.warnings);
    packaged
        .warnings
        .push(format!("Clean package prepared at {}", packaged.content_folder));

    Ok(packaged)
}

/// Write WorkshopItemId.txt next to SubModule.xml for future auto-detect.
pub fn write_published_file_id(req: WritePublishedFileIdRequest) -> Result<String, String> {
    if req.published_file_id == 0 {
        return Err("Published File ID must be greater than 0".to_string());
    }
    let mod_root = resolve_module_root(Path::new(req.mod_root.trim()))?;
    let path = mod_root.join("WorkshopItemId.txt");
    fs::write(&path, format!("{}\n", req.published_file_id))
        .map_err(|e| format!("Could not write WorkshopItemId.txt: {}", e))?;
    canonicalize_path(&path)
}

fn resolve_module_root(input: &Path) -> Result<PathBuf, String> {
    let path = if input.is_file() {
        // Selected SubModule.xml or similar
        if input
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("SubModule.xml"))
            .unwrap_or(false)
        {
            return input
                .parent()
                .map(|p| p.to_path_buf())
                .ok_or_else(|| "Invalid SubModule.xml path".to_string());
        }
        input
            .parent()
            .ok_or_else(|| "Invalid path".to_string())?
            .to_path_buf()
    } else {
        input.to_path_buf()
    };

    // Direct module root
    if path.join("SubModule.xml").is_file() {
        return Ok(path);
    }

    // `_Module` folder without xml (empty) → try sibling out/
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("_Module"))
        .unwrap_or(false)
    {
        if let Some(parent) = path.parent() {
            if let Some(found) = find_out_module(parent) {
                return Ok(found);
            }
            if path.join("SubModule.xml").is_file() {
                return Ok(path);
            }
        }
    }

    // Prefer built output: out/<ModuleId>/SubModule.xml
    if let Some(found) = find_out_module(&path) {
        return Ok(found);
    }

    // `_Module/SubModule.xml`
    let underscore = path.join("_Module");
    if underscore.join("SubModule.xml").is_file() {
        return Ok(underscore);
    }

    // One-level scan for SubModule.xml (single match only)
    if path.is_dir() {
        let mut matches = Vec::new();
        if let Ok(entries) = fs::read_dir(&path) {
            for entry in entries.flatten() {
                let child = entry.path();
                if child.is_dir() && child.join("SubModule.xml").is_file() {
                    // Skip pure source clones without bin if better match exists later
                    matches.push(child);
                }
            }
        }
        if matches.len() == 1 {
            return Ok(matches.remove(0));
        }
        if matches.len() > 1 {
            // Prefer ones that have bin/Win64_Shipping_Client
            let with_bin: Vec<_> = matches
                .iter()
                .filter(|p| p.join("bin/Win64_Shipping_Client").is_dir())
                .cloned()
                .collect();
            if with_bin.len() == 1 {
                return Ok(with_bin.into_iter().next().unwrap());
            }
            return Err(format!(
                "Multiple Bannerlord modules under {}. Select the module folder (or out/ModuleId) directly.",
                path.display()
            ));
        }
    }

    Err(format!(
        "Could not find SubModule.xml near {}. Select a Bannerlord module folder \
         (contains SubModule.xml) or a build output like out/ModuleId/.",
        input.display()
    ))
}

fn find_out_module(repo: &Path) -> Option<PathBuf> {
    let out = repo.join("out");
    if !out.is_dir() {
        return None;
    }
    let mut matches = Vec::new();
    if let Ok(entries) = fs::read_dir(&out) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() && child.join("SubModule.xml").is_file() {
                matches.push(child);
            }
        }
    }
    if matches.len() == 1 {
        return Some(matches.remove(0));
    }
    // Prefer folder with DLLs
    let with_dll: Vec<_> = matches
        .iter()
        .filter(|p| p.join("bin/Win64_Shipping_Client").is_dir())
        .cloned()
        .collect();
    if with_dll.len() == 1 {
        return Some(with_dll.into_iter().next().unwrap());
    }
    None
}

fn find_preview(mod_root: &Path) -> (Option<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let candidates = [
        mod_root.join("Image.png"),
        mod_root.join("Preview.png"),
        mod_root.join("preview.png"),
        mod_root.join("Preview.jpg"),
        mod_root.join("GUI").join("Image.png"),
    ];
    for c in candidates {
        if c.is_file() {
            return (Some(c.to_string_lossy().to_string()), warnings);
        }
    }

    // _Workshop next to module or parent (repo layout)
    for workshop in [
        mod_root.join("_Workshop"),
        mod_root
            .parent()
            .map(|p| p.join("_Workshop"))
            .unwrap_or_default(),
    ] {
        if !workshop.is_dir() {
            continue;
        }
        // Prefer Image.png
        let image = workshop.join("Image.png");
        if image.is_file() {
            return (Some(image.to_string_lossy().to_string()), warnings);
        }
        // First reasonably sized png (skip huge settings screenshots if smaller preview exists)
        if let Ok(entries) = fs::read_dir(&workshop) {
            let mut pngs: Vec<(u64, PathBuf)> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .and_then(|x| x.to_str())
                            .map(|x| x.eq_ignore_ascii_case("png") || x.eq_ignore_ascii_case("jpg"))
                            .unwrap_or(false)
                })
                .filter_map(|p| fs::metadata(&p).ok().map(|m| (m.len(), p)))
                .collect();
            pngs.sort_by_key(|(len, _)| *len);
            // Prefer under 1MB for Steam preview
            if let Some((_, p)) = pngs.iter().find(|(len, _)| *len <= 1_000_000) {
                return (Some(p.to_string_lossy().to_string()), warnings);
            }
            if let Some((len, p)) = pngs.first() {
                if *len > 1_000_000 {
                    warnings.push(format!(
                        "Preview candidate is {:.1} MB (Steam prefers ≤ 1 MB): {}",
                        *len as f64 / 1_000_000.0,
                        p.display()
                    ));
                }
                return (Some(p.to_string_lossy().to_string()), warnings);
            }
        }
    }

    (None, warnings)
}

fn find_published_file_id(mod_root: &Path) -> (Option<u64>, Option<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let search_roots = [
        mod_root.to_path_buf(),
        mod_root.join("_Workshop"),
        mod_root
            .parent()
            .map(|p| p.join("_Workshop"))
            .unwrap_or_default(),
        mod_root.parent().unwrap_or(mod_root).to_path_buf(),
    ];

    // WorkshopItemId.txt / PublishedFileId.txt
    for root in &search_roots {
        for name in ["WorkshopItemId.txt", "PublishedFileId.txt", "ItemId.txt"] {
            let p = root.join(name);
            if p.is_file() {
                if let Ok(raw) = fs::read_to_string(&p) {
                    let trimmed = raw.trim().trim_start_matches('\u{feff}');
                    if let Ok(id) = trimmed.parse::<u64>() {
                        if id > 0 {
                            return (
                                Some(id),
                                Some(p.to_string_lossy().to_string()),
                                warnings,
                            );
                        }
                    }
                }
            }
        }
    }

    // WorkshopUpdate.xml / .example with <ItemId Value="..."/>
    for root in &search_roots {
        for name in [
            "WorkshopUpdate.xml",
            "WorkshopUpdate.xml.example",
            "workshop.xml",
        ] {
            let p = root.join(name);
            if !p.is_file() {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(&p) {
                if let Some(id) = extract_item_id(&raw) {
                    if name.ends_with(".example") {
                        warnings.push(format!(
                            "Using ItemId from {} (example file) — verify before upload.",
                            p.display()
                        ));
                    }
                    return (Some(id), Some(p.to_string_lossy().to_string()), warnings);
                }
            }
        }
    }

    (None, None, warnings)
}

fn extract_item_id(xml: &str) -> Option<u64> {
    // <ItemId Value="2896410558"/> or <ItemId Value='...'/>
    let lower = xml.to_ascii_lowercase();
    let key = "itemid";
    let mut search = 0;
    while let Some(rel) = lower[search..].find(key) {
        let start = search + rel;
        let after = &xml[start..];
        if let Some(vpos) = after.to_ascii_lowercase().find("value") {
            let rest = &after[vpos..];
            if let Some(eq) = rest.find('=') {
                let rest = rest[eq + 1..].trim_start();
                let quote = rest.chars().next()?;
                if quote == '"' || quote == '\'' {
                    if let Some(end) = rest[1..].find(quote) {
                        let num = &rest[1..1 + end];
                        if let Ok(id) = num.parse::<u64>() {
                            if id > 0 {
                                return Some(id);
                            }
                        }
                    }
                }
            }
        }
        search = start + key.len();
    }
    None
}

fn find_readme_description(mod_root: &Path) -> Option<String> {
    let candidates = [
        mod_root.join("README.md"),
        mod_root.join("Readme.txt"),
        mod_root.join("readme.md"),
        mod_root
            .parent()
            .map(|p| p.join("README.md"))
            .unwrap_or_default(),
    ];
    for p in candidates {
        if p.is_file() {
            if let Ok(raw) = fs::read_to_string(&p) {
                let text = raw.trim();
                if !text.is_empty() {
                    // Cap for Steam description practicality
                    let capped = if text.len() > 7000 {
                        format!("{}…", &text[..7000])
                    } else {
                        text.to_string()
                    };
                    return Some(capped);
                }
            }
        }
    }
    None
}

fn build_fallback_description(
    name: &str,
    module_id: &str,
    version: Option<&str>,
    deps: &[String],
) -> String {
    let mut s = format!("{name}\n\nModule Id: {module_id}\n");
    if let Some(v) = version {
        s.push_str(&format!("Version: {v}\n"));
    }
    if !deps.is_empty() {
        s.push_str("\nDependencies:\n");
        for d in deps {
            s.push_str(&format!("- {d}\n"));
        }
    }
    s.push_str("\nMount & Blade II: Bannerlord module.");
    s
}

fn build_tags(singleplayer: bool, multiplayer: bool, version: Option<&str>) -> Vec<String> {
    let mut tags = vec!["Mod".to_string(), "Utility".to_string()];
    if singleplayer {
        tags.push("Singleplayer".to_string());
    }
    if multiplayer {
        tags.push("Multiplayer".to_string());
    }
    if let Some(v) = version {
        let cleaned = v.trim().trim_start_matches('v');
        if !cleaned.is_empty() {
            // Steam "Compatible Version" tags are game versions, not mod versions —
            // still useful as a freeform tag for authors.
            tags.push(format!("v{cleaned}"));
        }
    }
    tags
}

/// `<Name value="X"/>` style attributes used by SubModule.xml
fn attr_value(xml: &str, tag: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let tag_l = tag.to_ascii_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find(&format!("<{tag_l}")) {
        let start = search + rel;
        let after_tag = start + tag_l.len() + 1;
        let rest = &xml[after_tag..];
        let next = rest.chars().next()?;
        if !matches!(next, '>' | ' ' | '\t' | '\n' | '\r' | '/') {
            search = after_tag;
            continue;
        }
        // Find value="..."
        let segment = if let Some(gt) = rest.find('>') {
            &rest[..gt]
        } else {
            rest
        };
        let seg_l = segment.to_ascii_lowercase();
        if let Some(vpos) = seg_l.find("value") {
            let after_v = &segment[vpos + 5..].trim_start();
            if let Some(rest) = after_v.strip_prefix('=') {
                let rest = rest.trim_start();
                let q = rest.chars().next()?;
                if q == '"' || q == '\'' {
                    if let Some(end) = rest[1..].find(q) {
                        return Some(rest[1..1 + end].trim().to_string());
                    }
                }
            }
        }
        // Also support element text form <Name>X</Name>
        if let Some(gt) = rest.find('>') {
            let content_start = after_tag + gt + 1;
            let close = format!("</{tag_l}>");
            if let Some(end) = lower[content_start..].find(&close) {
                let content = xml[content_start..content_start + end].trim();
                if !content.is_empty() {
                    return Some(content.to_string());
                }
            }
        }
        search = after_tag;
    }
    None
}

fn bool_attr(xml: &str, tag: &str) -> Option<bool> {
    attr_value(xml, tag).and_then(|v| match v.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    })
}

fn depended_module_ids(xml: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let lower = xml.to_ascii_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find("<dependedmodule") {
        let start = search + rel;
        // avoid DependedModuleMetadata
        let slice = &lower[start..];
        if slice.starts_with("<dependedmodulemetadata") {
            search = start + 15;
            continue;
        }
        let after = &xml[start..];
        if let Some(gt) = after.find('>') {
            let open = &after[..=gt];
            // Id="..." or id='...'
            let open_l = open.to_ascii_lowercase();
            if let Some(ipos) = open_l.find("id") {
                let rest = open[ipos + 2..].trim_start();
                if let Some(rest) = rest.strip_prefix('=') {
                    let rest = rest.trim_start();
                    let q = rest.chars().next().unwrap_or(' ');
                    if q == '"' || q == '\'' {
                        if let Some(end) = rest[1..].find(q) {
                            let id = rest[1..1 + end].trim().to_string();
                            if !id.is_empty() && !ids.iter().any(|x| x == &id) {
                                ids.push(id);
                            }
                        }
                    }
                }
            }
            search = start + gt + 1;
        } else {
            break;
        }
    }
    ids
}

fn copy_filtered(src: &Path, dst: &Path, mod_root: &Path) -> Result<(), String> {
    let entries = fs::read_dir(src).map_err(|e| format!("{}: {}", src.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        let file_type = entry
            .file_type()
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        if file_type.is_symlink() {
            continue;
        }

        let rel = path.strip_prefix(mod_root).unwrap_or(&path);
        let top = rel
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or("");

        if file_type.is_dir() {
            if PACKAGE_EXCLUDE_DIRS
                .iter()
                .any(|d| name_str.eq_ignore_ascii_case(d) || top.eq_ignore_ascii_case(d))
            {
                continue;
            }
            // Keep bin/ (required) — never exclude
            let next_dst = dst.join(&name);
            fs::create_dir_all(&next_dst).map_err(|e| e.to_string())?;
            copy_filtered(&path, &next_dst, mod_root)?;
        } else if file_type.is_file() {
            if should_exclude_file(&name_str) {
                continue;
            }
            let next_dst = dst.join(&name);
            fs::copy(&path, &next_dst)
                .map_err(|e| format!("copy {} → {}: {}", path.display(), next_dst.display(), e))?;
        }
    }
    Ok(())
}

fn should_exclude_file(name: &str) -> bool {
    if PACKAGE_EXCLUDE_FILE_NAMES
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
    {
        return true;
    }
    if name.starts_with("._") {
        return true;
    }
    if let Some(ext) = Path::new(name).extension().and_then(|e| e.to_str()) {
        if PACKAGE_EXCLUDE_EXTENSIONS
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e))
        {
            return true;
        }
    }
    false
}

fn sanitize_folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "module".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

fn canonicalize_path(path: &Path) -> Result<String, String> {
    path.canonicalize()
        .map(|p| p.to_string_lossy().to_string())
        .or_else(|_| Ok(path.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_submodule_attrs() {
        let xml = r#"<?xml version="1.0"?>
<Module>
  <Name value="Bannerlord Auto Ammo Pickup" />
  <Id value="Bannerlord.AutoAmmoPickup" />
  <Version value="v1.1.0" />
  <SingleplayerModule value="true"/>
  <DependedModules>
    <DependedModule Id="Native" />
    <DependedModule Id="Bannerlord.Harmony"/>
  </DependedModules>
</Module>"#;
        assert_eq!(
            attr_value(xml, "Name").as_deref(),
            Some("Bannerlord Auto Ammo Pickup")
        );
        assert_eq!(
            attr_value(xml, "Id").as_deref(),
            Some("Bannerlord.AutoAmmoPickup")
        );
        assert_eq!(attr_value(xml, "Version").as_deref(), Some("v1.1.0"));
        assert_eq!(bool_attr(xml, "SingleplayerModule"), Some(true));
        assert_eq!(
            depended_module_ids(xml),
            vec!["Native".to_string(), "Bannerlord.Harmony".to_string()]
        );
    }

    #[test]
    fn parse_item_id() {
        let xml = r#"<GetItem><ItemId Value="2896410558"/></GetItem>"#;
        assert_eq!(extract_item_id(xml), Some(2896410558));
    }

    #[test]
    fn detect_auto_ammo_if_present() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../Bannerlord.AutoAmmoPickup/out/Bannerlord.AutoAmmoPickup");
        let path = if path.join("SubModule.xml").is_file() {
            path
        } else {
            PathBuf::from("/Users/dongxuli/documents/workspace/Bannerlord.AutoAmmoPickup/out/Bannerlord.AutoAmmoPickup")
        };
        if !path.join("SubModule.xml").is_file() {
            return;
        }
        let info = detect_bannerlord_mod(path.to_string_lossy().to_string()).expect("detect");
        assert!(info.name.to_ascii_lowercase().contains("ammo"));
        assert_eq!(info.module_id, "Bannerlord.AutoAmmoPickup");
        assert!(!info.depended_modules.is_empty());
    }
}

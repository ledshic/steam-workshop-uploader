use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// RimWorld Steam AppID.
pub const RIMWORLD_APP_ID: u32 = 294100;

/// Paths that should not be uploaded to the Workshop (dev / VCS junk).
/// Matching is case-insensitive.
const PACKAGE_EXCLUDE_DIRS: &[&str] = &[
    ".git",
    ".github",
    ".vs",
    ".vscode",
    ".idea",
    "Source",
    "bin",
    "obj",
    "node_modules",
    "_decompile_tmp",
    // Common local-only game reference mounts (often symlinks; also excluded by name).
    "RimWorld-Data",
    "RimWorld-XMLs",
    "RimWorldData",
    "GameData",
];

const PACKAGE_EXCLUDE_FILE_NAMES: &[&str] = &[
    ".DS_Store",
    ".gitignore",
    ".gitattributes",
    "Directory.Build.props",
    "Directory.Build.targets",
    "Thumbs.db",
    "desktop.ini",
    // Local-only backup of the original cover; never upload the multi-MB original.
    "Preview.original.png",
    "Preview.original.jpg",
    "Preview.original.jpeg",
];

/// Dev / decompile / tooling files — never ship these to Workshop.
const PACKAGE_EXCLUDE_EXTENSIONS: &[&str] = &[
    "sln",
    "csproj",
    "user",
    "suo",
    "pdb",
    "md",
    "code-workspace",
    "cs",  // C# source + decompiled dumps
    "vdf", // local steamcmd / workshop helper configs
];

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RimWorldModInfo {
    /// Canonical mod root (folder that contains About/).
    pub mod_root: String,
    /// Folder that should be uploaded (same as mod_root unless a clean package was prepared).
    pub content_folder: String,
    pub about_xml_path: String,
    pub name: String,
    pub description: String,
    pub author: Option<String>,
    pub package_id: Option<String>,
    pub url: Option<String>,
    pub supported_versions: Vec<String>,
    pub preview_file: Option<String>,
    pub published_file_id: Option<u64>,
    pub published_file_id_path: Option<String>,
    pub tags: Vec<String>,
    pub warnings: Vec<String>,
    pub detected_files: Vec<String>,
    /// True when content_folder is a temporary clean package.
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

/// Detect a RimWorld mod from a selected path (mod root or About folder).
pub fn detect_rimworld_mod(path: String) -> Result<RimWorldModInfo, String> {
    let input = PathBuf::from(path.trim());
    if !input.exists() {
        return Err(format!("Path not found: {}", input.display()));
    }

    let mod_root = resolve_mod_root(&input)?;
    let about_dir = mod_root.join("About");
    let about_xml = about_dir.join("About.xml");
    if !about_xml.is_file() {
        return Err(format!(
            "Not a RimWorld mod: missing About/About.xml under {}",
            mod_root.display()
        ));
    }

    let about_raw = fs::read_to_string(&about_xml)
        .map_err(|e| format!("Could not read About.xml: {}", e))?;
    // Strip UTF-8 BOM if present
    let about_raw = about_raw.strip_prefix('\u{feff}').unwrap_or(&about_raw);

    let name = extract_xml_text(about_raw, "name")
        .map(|s| decode_xml_entities(&s).trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "About.xml is missing a <name> element".to_string())?;

    let description = extract_xml_text(about_raw, "description")
        .map(|s| decode_xml_entities(&s).trim().to_string())
        .unwrap_or_default();

    let author = extract_xml_text(about_raw, "author")
        .or_else(|| extract_authors_list(about_raw))
        .map(|s| decode_xml_entities(&s).trim().to_string())
        .filter(|s| !s.is_empty());

    let package_id = extract_xml_text(about_raw, "packageId")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let url = extract_xml_text(about_raw, "url")
        .map(|s| decode_xml_entities(&s).trim().to_string())
        .filter(|s| !s.is_empty());

    let supported_versions = extract_list_items(about_raw, "supportedVersions");

    let mut warnings = Vec::new();
    let mut detected_files = vec![about_xml.display().to_string()];

    if description.is_empty() {
        warnings.push(
            "About.xml has an empty <description>; Workshop description will be blank.".to_string(),
        );
    }

    let preview_file = find_preview_image(&about_dir, &mod_root);
    if let Some(ref preview) = preview_file {
        detected_files.push(preview.clone());
        if let Ok(meta) = fs::metadata(preview) {
            // Steam Workshop preview soft limit is ~1 MB
            if meta.len() > 1_000_000 {
                warnings.push(format!(
                    "Preview image is {:.1} MB (Steam prefers ≤ 1 MB). Upload may still work.",
                    meta.len() as f64 / 1_000_000.0
                ));
            }
        }
    } else {
        warnings.push(
            "No preview image found (About/Preview.png|jpg|jpeg or About/ModIcon.png).".to_string(),
        );
    }

    let published_file_id_path = about_dir.join("PublishedFileId.txt");
    let published_file_id = if published_file_id_path.is_file() {
        detected_files.push(published_file_id_path.display().to_string());
        match fs::read_to_string(&published_file_id_path) {
            Ok(raw) => {
                let trimmed = raw.trim().trim_start_matches('\u{feff}');
                match trimmed.parse::<u64>() {
                    Ok(id) if id > 0 => Some(id),
                    Ok(_) => {
                        warnings.push("PublishedFileId.txt contains 0; treating as new upload.".to_string());
                        None
                    }
                    Err(_) => {
                        warnings.push(format!(
                            "Could not parse PublishedFileId.txt ({:?}); treating as new upload.",
                            trimmed
                        ));
                        None
                    }
                }
            }
            Err(e) => {
                warnings.push(format!("Could not read PublishedFileId.txt: {}", e));
                None
            }
        }
    } else {
        None
    };

    // Flag common junk / symlinks that would bloat the upload if packaging is skipped.
    if let Ok(entries) = fs::read_dir(&mod_root) {
        let mut warned: Vec<String> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_link = entry
                .file_type()
                .map(|t| t.is_symlink())
                .unwrap_or(false);
            let is_dir = path.is_dir(); // may follow symlink; used only for messaging

            if is_link {
                warnings.push(format!(
                    "Found symlink '{}' — clean packaging will skip it (not uploaded).",
                    name
                ));
                continue;
            }

            if !is_dir {
                if should_exclude_file(&name) {
                    warnings.push(format!(
                        "Found '{}' — clean packaging will exclude this file.",
                        name
                    ));
                }
                continue;
            }

            let is_excluded = PACKAGE_EXCLUDE_DIRS
                .iter()
                .any(|d| name.eq_ignore_ascii_case(d))
                || name_looks_like_decompile(&name);
            if is_excluded && !warned.iter().any(|w| w.eq_ignore_ascii_case(&name)) {
                warnings.push(format!(
                    "Found '{}' — enable clean packaging to exclude it from upload.",
                    name
                ));
                warned.push(name);
            }
        }
    }

    let tags = build_tags(&supported_versions);

    let mod_root_str = canonicalize_path(&mod_root)?;
    let about_xml_str = canonicalize_path(&about_xml)?;

    Ok(RimWorldModInfo {
        content_folder: mod_root_str.clone(),
        mod_root: mod_root_str,
        about_xml_path: about_xml_str,
        name,
        description,
        author,
        package_id,
        url,
        supported_versions,
        preview_file: preview_file.and_then(|p| canonicalize_path(Path::new(&p)).ok()),
        published_file_id,
        published_file_id_path: if published_file_id_path.exists() {
            canonicalize_path(&published_file_id_path).ok()
        } else {
            Some(published_file_id_path.to_string_lossy().to_string())
        },
        tags,
        warnings,
        detected_files,
        is_packaged: false,
    })
}

/// Copy a RimWorld mod into a clean temporary package, excluding Source / VCS / build artifacts.
pub fn prepare_rimworld_package(req: PreparePackageRequest) -> Result<RimWorldModInfo, String> {
    let mut info = detect_rimworld_mod(req.mod_root)?;
    let mod_root = PathBuf::from(&info.mod_root);

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let package_name = sanitize_folder_name(&info.name);
    let package_root = std::env::temp_dir().join(format!(
        "rimworld_workshop_{}_{}",
        package_name, stamp
    ));

    if package_root.exists() {
        fs::remove_dir_all(&package_root)
            .map_err(|e| format!("Could not clear previous package dir: {}", e))?;
    }
    fs::create_dir_all(&package_root)
        .map_err(|e| format!("Could not create package dir: {}", e))?;

    copy_filtered(&mod_root, &package_root, &mod_root)
        .map_err(|e| format!("Failed while packaging mod: {}", e))?;

    // Re-detect against the packaged folder so paths point at the clean copy.
    let mut packaged = detect_rimworld_mod(package_root.to_string_lossy().to_string())?;
    // Keep original mod_root so PublishedFileId.txt can be written back after upload.
    packaged.mod_root = info.mod_root.clone();
    packaged.published_file_id_path = info.published_file_id_path.clone();
    packaged.published_file_id = info.published_file_id.or(packaged.published_file_id);
    packaged.is_packaged = true;
    packaged.warnings.append(&mut info.warnings);
    packaged
        .warnings
        .push(format!("Clean package prepared at {}", packaged.content_folder));
    // Prefer preview from package if present; already set by detect.

    Ok(packaged)
}

/// Write About/PublishedFileId.txt after a successful first upload so later updates auto-detect.
pub fn write_published_file_id(req: WritePublishedFileIdRequest) -> Result<String, String> {
    if req.published_file_id == 0 {
        return Err("Published File ID must be greater than 0".to_string());
    }

    let mod_root = PathBuf::from(req.mod_root.trim());
    let mod_root = resolve_mod_root(&mod_root)?;
    let about_dir = mod_root.join("About");
    if !about_dir.is_dir() {
        return Err(format!("About folder not found under {}", mod_root.display()));
    }

    let path = about_dir.join("PublishedFileId.txt");
    fs::write(&path, format!("{}\n", req.published_file_id))
        .map_err(|e| format!("Could not write PublishedFileId.txt: {}", e))?;

    canonicalize_path(&path)
}

fn resolve_mod_root(input: &Path) -> Result<PathBuf, String> {
    let path = if input.is_file() {
        input
            .parent()
            .ok_or_else(|| "Invalid path".to_string())?
            .to_path_buf()
    } else {
        input.to_path_buf()
    };

    // Selected About/ directly
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.eq_ignore_ascii_case("About"))
        .unwrap_or(false)
        && path.join("About.xml").is_file()
    {
        return path
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "About folder has no parent".to_string());
    }

    // Selected mod root
    if path.join("About").join("About.xml").is_file() {
        return Ok(path);
    }

    // Selected a file inside About/
    if path.join("About.xml").is_file() {
        return path
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "Could not resolve mod root from About path".to_string());
    }

    // Shallow search: child/*/About/About.xml (one level)
    if path.is_dir() {
        if let Ok(entries) = fs::read_dir(&path) {
            let mut matches = Vec::new();
            for entry in entries.flatten() {
                let child = entry.path();
                if child.is_dir() && child.join("About").join("About.xml").is_file() {
                    matches.push(child);
                }
            }
            if matches.len() == 1 {
                return Ok(matches.remove(0));
            }
            if matches.len() > 1 {
                return Err(format!(
                    "Multiple RimWorld mods found under {}. Please select the mod folder directly.",
                    path.display()
                ));
            }
        }
    }

    Err(format!(
        "Could not find About/About.xml near {}. Select the RimWorld mod root folder.",
        input.display()
    ))
}

fn find_preview_image(about_dir: &Path, mod_root: &Path) -> Option<String> {
    let candidates = [
        about_dir.join("Preview.png"),
        about_dir.join("Preview.jpg"),
        about_dir.join("Preview.jpeg"),
        about_dir.join("preview.png"),
        about_dir.join("preview.jpg"),
        about_dir.join("ModIcon.png"),
        about_dir.join("modicon.png"),
        mod_root.join("Preview.png"),
        mod_root.join("preview.png"),
    ];

    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn build_tags(supported_versions: &[String]) -> Vec<String> {
    let mut tags = vec!["Mod".to_string()];
    for v in supported_versions {
        let cleaned = v.trim().to_string();
        if !cleaned.is_empty() && !tags.iter().any(|t| t == &cleaned) {
            tags.push(cleaned);
        }
    }
    tags
}

fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    // Prefer CDATA form
    let cdata_open = format!("<{}>", tag);
    let cdata_open_ws = format!("<{} ", tag);

    let lower = xml.to_ascii_lowercase();
    let tag_lower = tag.to_ascii_lowercase();
    let open_pat = format!("<{}", tag_lower);
    let close_pat = format!("</{}>", tag_lower);

    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find(&open_pat) {
        let start = search_from + rel;
        let after_tag = start + open_pat.len();
        let rest = &xml[after_tag..];
        // Ensure we matched a real element (next char is > or whitespace or /)
        let next = rest.chars().next();
        if !matches!(next, Some('>') | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('/'))
        {
            search_from = after_tag;
            continue;
        }

        // Self-closing
        if let Some(end_of_open) = rest.find('>') {
            let open_inner = &rest[..end_of_open];
            if open_inner.ends_with('/') {
                return Some(String::new());
            }
            let content_start = after_tag + end_of_open + 1;
            if let Some(close_rel) = lower[content_start..].find(&close_pat) {
                let content = &xml[content_start..content_start + close_rel];
                return Some(strip_cdata(content).trim().to_string());
            }
        }
        break;
    }

    // Fallback: case-sensitive exact tags used by most mods
    if let Some(start) = xml.find(&cdata_open) {
        let content_start = start + cdata_open.len();
        let close = format!("</{}>", tag);
        if let Some(end) = xml[content_start..].find(&close) {
            return Some(strip_cdata(&xml[content_start..content_start + end]).trim().to_string());
        }
    }
    if let Some(start) = xml.find(&cdata_open_ws) {
        if let Some(gt) = xml[start..].find('>') {
            let content_start = start + gt + 1;
            let close = format!("</{}>", tag);
            if let Some(end) = xml[content_start..].find(&close) {
                return Some(strip_cdata(&xml[content_start..content_start + end]).trim().to_string());
            }
        }
    }

    None
}

fn extract_list_items(xml: &str, parent_tag: &str) -> Vec<String> {
    let Some(parent) = extract_xml_text(xml, parent_tag) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    let lower = parent.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find("<li") {
        let start = search_from + rel;
        let after = start + 3;
        let rest = &parent[after..];
        let next = rest.chars().next();
        if !matches!(next, Some('>') | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('/'))
        {
            search_from = after;
            continue;
        }
        if let Some(gt) = rest.find('>') {
            let open_inner = &rest[..gt];
            if open_inner.ends_with('/') {
                search_from = after + gt + 1;
                continue;
            }
            let content_start = after + gt + 1;
            if let Some(close_rel) = lower[content_start..].find("</li>") {
                let value = parent[content_start..content_start + close_rel].trim();
                if !value.is_empty() {
                    items.push(decode_xml_entities(value));
                }
                search_from = content_start + close_rel + 5;
                continue;
            }
        }
        break;
    }
    items
}

fn extract_authors_list(xml: &str) -> Option<String> {
    let authors = extract_list_items(xml, "authors");
    if authors.is_empty() {
        None
    } else {
        Some(authors.join(", "))
    }
}

fn strip_cdata(content: &str) -> String {
    let trimmed = content.trim();
    if let Some(inner) = trimmed
        .strip_prefix("<![CDATA[")
        .and_then(|s| s.strip_suffix("]]>"))
    {
        inner.to_string()
    } else if trimmed.starts_with("<![CDATA[") {
        // multiline / whitespace variants
        let without_open = trimmed.replacen("<![CDATA[", "", 1);
        without_open
            .strip_suffix("]]>")
            .unwrap_or(&without_open)
            .to_string()
    } else {
        content.to_string()
    }
}

fn decode_xml_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        let mut entity = String::new();
        while let Some(&ch) = chars.peek() {
            if ch == ';' {
                chars.next();
                break;
            }
            entity.push(ch);
            chars.next();
            if entity.len() > 12 {
                break;
            }
        }
        match entity.as_str() {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            "nbsp" => out.push(' '),
            other if other.starts_with('#') => {
                let num = &other[1..];
                let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X'))
                {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    num.parse::<u32>().ok()
                };
                if let Some(cp) = code.and_then(char::from_u32) {
                    out.push(cp);
                } else {
                    out.push('&');
                    out.push_str(other);
                    out.push(';');
                }
            }
            other => {
                out.push('&');
                out.push_str(other);
                if !other.is_empty() {
                    out.push(';');
                }
            }
        }
    }
    out
}

fn copy_filtered(src: &Path, dst: &Path, mod_root: &Path) -> Result<(), String> {
    let entries = fs::read_dir(src).map_err(|e| format!("{}: {}", src.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Never follow or copy symlinks (e.g. RimWorld-Data / RimWorld-XMLs game mounts).
        let file_type = entry
            .file_type()
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        if file_type.is_symlink() {
            continue;
        }

        // Relative path components from mod root for top-level dir checks
        let rel = path.strip_prefix(mod_root).unwrap_or(&path);
        let top = rel
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or("");

        if file_type.is_dir() {
            if PACKAGE_EXCLUDE_DIRS.iter().any(|d| {
                name_str.eq_ignore_ascii_case(d) || top.eq_ignore_ascii_case(d)
            }) {
                continue;
            }
            // Skip decompile leftovers (dirs and naming conventions)
            if name_looks_like_decompile(&name_str) {
                continue;
            }
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
        // Other special file types (sockets, etc.) are ignored.
    }
    Ok(())
}

fn name_looks_like_decompile(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("_decompile")
        || lower.contains("decompile_tmp")
        || lower.contains("_decompiled")
        || lower.starts_with("decompiled")
}

fn should_exclude_file(name: &str) -> bool {
    if PACKAGE_EXCLUDE_FILE_NAMES
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n))
    {
        return true;
    }
    if name_looks_like_decompile(name) {
        return true;
    }
    if name.starts_with("._") {
        return true;
    }
    // Multi-part extensions first (e.g. dll.config)
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".dll.config") {
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
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "mod".to_string()
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
    fn parse_name_and_description() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ModMetaData>
  <name>Test Mod</name>
  <description>Hello &amp; welcome

Line 2</description>
  <supportedVersions>
    <li>1.5</li>
    <li>1.6</li>
  </supportedVersions>
</ModMetaData>"#;
        assert_eq!(extract_xml_text(xml, "name").as_deref(), Some("Test Mod"));
        let desc = extract_xml_text(xml, "description").unwrap();
        assert!(desc.contains("Hello &amp; welcome") || desc.contains("Hello"));
        assert_eq!(
            decode_xml_entities(&desc).contains("Hello & welcome"),
            true
        );
        assert_eq!(extract_list_items(xml, "supportedVersions"), vec!["1.5", "1.6"]);
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_mod_path() -> Option<PathBuf> {
        let candidates = [
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../easyrim"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../easyrim"),
        ];
        candidates.into_iter().find(|p| p.join("About/About.xml").is_file())
    }

    #[test]
    fn detect_sample_mod_if_present() {
        let Some(path) = sample_mod_path() else {
            return;
        };
        let info = detect_rimworld_mod(path.to_string_lossy().to_string()).expect("detect");
        assert!(!info.name.is_empty());
        assert!(!info.description.is_empty());
        assert!(info.preview_file.is_some());
        assert_eq!(info.tags[0], "Mod");
    }

    #[test]
    fn prepare_package_sample_mod_if_present() {
        let Some(path) = sample_mod_path() else {
            return;
        };
        let info = prepare_rimworld_package(PreparePackageRequest {
            mod_root: path.to_string_lossy().to_string(),
        })
        .expect("package");
        assert!(info.is_packaged);
        let pkg = PathBuf::from(&info.content_folder);
        assert!(pkg.join("About/About.xml").is_file());
        // Dev / decompile / game mounts must never land in the package.
        assert!(!pkg.join("Source").exists());
        assert!(!pkg.join("RimWorld-Data").exists());
        assert!(!pkg.join("RimWorld-XMLs").exists());
        assert!(!pkg.join("_decompile_tmp_HediffDef.cs").exists());
        assert!(!pkg.join("easymode.sln").exists());
        assert!(!pkg.join("easyrim.vdf").exists());
        // No loose .cs anywhere in package
        let mut cs_files = Vec::new();
        fn walk_cs(dir: &PathBuf, out: &mut Vec<PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        walk_cs(&p, out);
                    } else if p.extension().and_then(|x| x.to_str()) == Some("cs") {
                        out.push(p);
                    }
                }
            }
        }
        walk_cs(&pkg, &mut cs_files);
        assert!(
            cs_files.is_empty(),
            "package must not contain .cs files: {:?}",
            cs_files
        );
        // No symlinks in package
        fn walk_links(dir: &PathBuf, out: &mut Vec<PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                        out.push(p.clone());
                    }
                    if p.is_dir() && !e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                        walk_links(&p, out);
                    }
                }
            }
        }
        let mut links = Vec::new();
        walk_links(&pkg, &mut links);
        assert!(links.is_empty(), "package must not contain symlinks: {:?}", links);
        // Expected content dirs present when source has them
        assert!(pkg.join("Assemblies").is_dir() || pkg.join("Defs").is_dir());
    }

    #[test]
    fn exclude_helpers() {
        assert!(should_exclude_file("_decompile_tmp_HediffDef.cs"));
        assert!(should_exclude_file("Foo.cs"));
        assert!(should_exclude_file("easyrim.vdf"));
        assert!(name_looks_like_decompile("_decompile_tmp"));
        assert!(!should_exclude_file("About.xml"));
        assert!(!should_exclude_file("Preview.png"));
        assert!(!should_exclude_file("EasyMode.dll"));
    }
}

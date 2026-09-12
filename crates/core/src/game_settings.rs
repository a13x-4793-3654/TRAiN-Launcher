//! Transfer client preferences without changing the destination's pack selection.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::paths::LauncherPaths;
use crate::CoreError;

const MAX_SETTINGS_BYTES: u64 = 8 * 1024 * 1024;
const PRESERVED_KEYS: &[&str] = &["resourcePacks", "incompatibleResourcePacks", "lastServer"];

fn failure(message: impl Into<String>) -> CoreError {
    CoreError::GameSettings(message.into())
}

pub fn minecraft_version(version_id: &str, launcher_root: &Path) -> Result<String, CoreError> {
    let paths = LauncherPaths::new(launcher_root);
    let mut current = version_id.trim().to_string();
    let mut visited = HashSet::new();
    for _ in 0..8 {
        if current.is_empty()
            || matches!(
                current.as_str(),
                "." | ".." | "latest-release" | "latest-snapshot"
            )
            || current.contains(['/', '\\', ':', '\0'])
        {
            return Err(failure(format!(
                "Minecraftバージョン「{current}」を確定できません。latest-release等ではなく具体的なバージョンを指定してください。"
            )));
        }
        if !visited.insert(current.clone()) {
            return Err(failure("バージョン情報の継承が循環しています"));
        }
        let path = paths.version_json_path(&current);
        let text = match read_limited(&path) {
            Ok(text) => text,
            Err(CoreError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(current);
            }
            Err(err) => return Err(err),
        };
        let value: serde_json::Value = serde_json::from_str(&text)?;
        match value.get("inheritsFrom") {
            Some(serde_json::Value::String(parent)) => current = parent.clone(),
            None | Some(serde_json::Value::Null) => return Ok(current),
            _ => return Err(failure("バージョン情報のinheritsFromが不正です")),
        }
    }
    Err(failure("バージョン情報の継承が深すぎます"))
}

fn read_limited(path: &Path) -> Result<String, CoreError> {
    if !fs::metadata(path)?.is_file() {
        return Err(failure(format!(
            "通常のファイルではありません: {}",
            path.display()
        )));
    }
    let mut text = String::new();
    fs::File::open(path)?
        .take(MAX_SETTINGS_BYTES + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(failure(format!(
            "設定ファイルが大きすぎます: {}",
            path.display()
        )));
    }
    Ok(text)
}

pub fn read_options(game_dir: &Path) -> Result<String, CoreError> {
    let path = game_dir.join("options.txt");
    let metadata = fs::symlink_metadata(&path)?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = metadata.file_type().is_symlink();
    if !metadata.is_file() || reparse {
        return Err(failure(format!(
            "通常のoptions.txt以外は取り込めません: {}",
            path.display()
        )));
    }
    read_limited(&path)
}

fn parse_options(text: &str) -> Result<Vec<(String, String)>, CoreError> {
    if text.len() as u64 > MAX_SETTINGS_BYTES || text.contains('\0') {
        return Err(failure("options.txtのサイズまたは内容が不正です"));
    }
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut indices = HashMap::new();
    for (number, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (key, value) = line.split_once(':').ok_or_else(|| {
            failure(format!(
                "options.txtの{}行目が設定形式ではありません",
                number + 1
            ))
        })?;
        if key.is_empty() || key.trim() != key {
            return Err(failure(format!(
                "options.txtの{}行目の設定名が不正です",
                number + 1
            )));
        }
        if let Some(index) = indices.get(key).copied() {
            entries[index] = (key.to_string(), value.to_string());
        } else {
            indices.insert(key.to_string(), entries.len());
            entries.push((key.to_string(), value.to_string()));
        }
    }
    Ok(entries)
}

fn data_version(entries: &[(String, String)]) -> Result<Option<u32>, CoreError> {
    entries
        .iter()
        .find(|(key, _)| key == "version")
        .map(|(_, value)| {
            value
                .parse()
                .map_err(|_| failure("options.txtのversionが不正です"))
        })
        .transpose()
}

fn merge(source: &str, target: &str) -> Result<(String, usize), CoreError> {
    let source = parse_options(source)?;
    let mut target = parse_options(target)?;
    if let (Some(source_version), Some(target_version)) =
        (data_version(&source)?, data_version(&target)?)
    {
        if source_version != target_version {
            return Err(failure(
                "設定ファイルの形式バージョンが異なります。両方を同じMinecraftバージョンで一度起動・終了してから取り込んでください。",
            ));
        }
    }
    let mut imported = 0;
    for (key, value) in source {
        if PRESERVED_KEYS.contains(&key.as_str()) {
            continue;
        }
        if key != "version" {
            imported += 1;
        }
        if let Some(entry) = target.iter_mut().find(|(existing, _)| existing == &key) {
            *entry = (key, value);
        } else {
            target.push((key, value));
        }
    }
    if imported == 0 {
        return Err(failure(
            "取り込み元にキー割り当て・音量・描画などの設定がありません",
        ));
    }
    let rendered = target
        .into_iter()
        .map(|(key, value)| format!("{key}:{value}\n"))
        .collect();
    Ok((rendered, imported))
}

pub fn merge_options(source: &str, target: &str) -> Result<String, CoreError> {
    merge(source, target).map(|(text, _)| text)
}

pub fn has_importable_settings(text: &str) -> Result<bool, CoreError> {
    let entries = parse_options(text)?;
    data_version(&entries)?;
    Ok(entries
        .iter()
        .any(|(key, _)| key != "version" && !PRESERVED_KEYS.contains(&key.as_str())))
}

pub fn validate_destination(game_dir: &Path) -> Result<PathBuf, CoreError> {
    fs::create_dir_all(game_dir)?;
    let directory = fs::canonicalize(game_dir)?;
    let current = fs::canonicalize(std::env::current_dir()?)?;
    let home = dirs::home_dir().map(fs::canonicalize).transpose()?;
    if directory.parent().is_none()
        || current.starts_with(&directory)
        || home.as_ref() == Some(&directory)
        || directory.join(".git").exists()
    {
        return Err(failure(
            "このフォルダーはゲーム設定の書き込み先に指定できません",
        ));
    }
    Ok(directory)
}

pub fn write_options(game_dir: &Path, content: &str) -> Result<(), CoreError> {
    let directory = validate_destination(game_dir)?;
    let path = directory.join("options.txt");
    let permissions = match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            read_options(&directory)?;
            if metadata.permissions().readonly() {
                return Err(failure("移行先のoptions.txtが読み取り専用です"));
            }
            Some(metadata.permissions())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.into()),
    };
    let mut file = tempfile::Builder::new()
        .prefix(".train-launcher-options-")
        .tempfile_in(&directory)?;
    file.write_all(content.as_bytes())?;
    if let Some(permissions) = permissions {
        file.as_file().set_permissions(permissions)?;
    }
    file.as_file().sync_all()?;
    file.persist(path).map_err(|err| err.error)?;
    Ok(())
}

/// Callers validate profile versions, lock game activity, and create a safety
/// backup before invoking this filesystem operation.
pub fn import_options(source_game_dir: &Path, target_game_dir: &Path) -> Result<usize, CoreError> {
    let source = read_options(source_game_dir)?;
    let target_directory = validate_destination(target_game_dir)?;
    if fs::canonicalize(source_game_dir)? == target_directory {
        return Err(failure(
            "同じゲームフォルダーへ設定を取り込むことはできません",
        ));
    }
    let target = match read_options(&target_directory) {
        Ok(text) => text,
        Err(CoreError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };
    let (merged, imported) = merge(&source, &target)?;
    write_options(&target_directory, &merged)?;
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfers_preferences_preserving_destination_pack_and_server_choices() {
        let source = "version:3955\nkey_key.forward:key.keyboard.r\nsoundCategory_master:0.4\nfov:0.8\nresourcePacks:[\"source.zip\"]\nincompatibleResourcePacks:[\"source.zip\"]\nlastServer:source\n";
        let target = "version:3955\nkey_key.forward:key.keyboard.w\nresourcePacks:[\"target.zip\"]\nincompatibleResourcePacks:[]\nlastServer:target\nkey_key.targetmod:key.keyboard.k\n";
        let (text, count) = merge(source, target).unwrap();
        assert_eq!(count, 3);
        assert!(text.contains("key_key.forward:key.keyboard.r\n"));
        assert!(text.contains("soundCategory_master:0.4\n"));
        assert!(text.contains("resourcePacks:[\"target.zip\"]"));
        assert!(text.contains("lastServer:target"));
        assert!(text.contains("key_key.targetmod:key.keyboard.k"));
        assert!(!text.contains("source.zip"));
    }

    #[test]
    fn rejects_mismatched_data_versions_and_invalid_settings() {
        assert!(merge_options("version:3955\nfov:1\n", "version:3465\n").is_err());
        for bad in [
            "garbage",
            "key:bad\0",
            "version:invalid\nfov:1\n",
            "resourcePacks:[]\n",
        ] {
            assert!(merge_options(bad, "").is_err());
        }
    }

    #[test]
    fn distinguishes_absent_preferences_from_invalid_settings() {
        for text in [
            "",
            "\u{feff}\n",
            "version:3955\nresourcePacks:[]\nincompatibleResourcePacks:[]\nlastServer:target\n",
        ] {
            assert!(!has_importable_settings(text).unwrap());
        }
        assert!(has_importable_settings("version:3955\nfov:1\n").unwrap());
        assert!(has_importable_settings("garbage").is_err());
        assert!(has_importable_settings("version:invalid\n").is_err());
    }

    #[test]
    fn handles_bom_crlf_colons_and_duplicates_without_losing_overrides() {
        let text = merge_options(
            "\u{feff}version:3955\r\nkey_key.jump:key.keyboard.z\r\nkey_key.jump:key.keyboard.x\r\ncustom:a:b\r\n",
            "key_key.jump:key.keyboard.space\nkey_key.jump:key.keyboard.q\n",
        ).unwrap();
        assert_eq!(text.matches("key_key.jump:").count(), 1);
        assert!(text.contains("key_key.jump:key.keyboard.x"));
        assert!(text.contains("custom:a:b\n"));
        assert!(text.contains("version:3955\n"));
    }

    #[test]
    fn import_is_atomic_and_leaves_source_and_other_game_data_unchanged() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        fs::write(source.path().join("options.txt"), "version:3955\nfov:0.5\n").unwrap();
        fs::write(
            target.path().join("options.txt"),
            "version:3955\nfov:1\nresourcePacks:[]\n",
        )
        .unwrap();
        fs::write(target.path().join("world.data"), b"unchanged").unwrap();
        assert_eq!(import_options(source.path(), target.path()).unwrap(), 1);
        assert_eq!(
            read_options(source.path()).unwrap(),
            "version:3955\nfov:0.5\n"
        );
        assert_eq!(
            fs::read(target.path().join("world.data")).unwrap(),
            b"unchanged"
        );
        assert!(read_options(target.path()).unwrap().contains("fov:0.5"));
        assert!(import_options(source.path(), source.path()).is_err());
        fs::write(source.path().join("options.txt"), "version:1\nfov:0\n").unwrap();
        let before = read_options(target.path()).unwrap();
        assert!(import_options(source.path(), target.path()).is_err());
        assert_eq!(read_options(target.path()).unwrap(), before);
    }

    #[test]
    fn resolves_mod_loader_inheritance_offline_and_rejects_aliases_and_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let paths = LauncherPaths::new(dir.path());
        fs::create_dir_all(paths.version_dir("fabric-test")).unwrap();
        fs::write(
            paths.version_json_path("fabric-test"),
            r#"{"inheritsFrom":"1.21.1"}"#,
        )
        .unwrap();
        assert_eq!(
            minecraft_version("fabric-test", dir.path()).unwrap(),
            "1.21.1"
        );
        assert_eq!(minecraft_version("1.21.1", dir.path()).unwrap(), "1.21.1");
        assert!(minecraft_version("latest-release", dir.path()).is_err());
        assert!(minecraft_version("../outside", dir.path()).is_err());
        fs::write(
            paths.version_json_path("fabric-test"),
            r#"{"inheritsFrom":"fabric-test"}"#,
        )
        .unwrap();
        assert!(minecraft_version("fabric-test", dir.path()).is_err());
    }

    #[test]
    fn server_resource_pack_sync_preserves_imported_preferences() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        fs::write(
            source.path().join("options.txt"),
            "version:3955\nkey_key.forward:key.keyboard.r\nfov:0.5\nsoundCategory_master:0.4\n",
        )
        .unwrap();
        import_options(source.path(), target.path()).unwrap();
        let mut previously_enabled = Vec::new();
        crate::resource_pack_options::apply(
            target.path(),
            &["server-pack.zip".to_string()],
            &mut previously_enabled,
        )
        .unwrap();
        let text = read_options(target.path()).unwrap();
        assert!(text.contains("key_key.forward:key.keyboard.r"));
        assert!(text.contains("fov:0.5"));
        assert!(text.contains("soundCategory_master:0.4"));
        assert!(text.contains("file/server-pack.zip"));
    }
}

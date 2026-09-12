use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use train_launcher_core::backup::{
    self, BackupInfo, BackupList, BackupProfile, BackupScope, RestoreResult,
};
use train_launcher_core::game_settings;
use train_launcher_core::profile::{self, Profile, ProfileSource};
use train_launcher_core::{paths, CoreError};

use crate::game_activity::{ActivityStatus, GameActivity};

pub(super) fn shared_root() -> Result<PathBuf, String> {
    let settings = train_launcher_core::settings::load_settings().map_err(|err| err.to_string())?;
    Ok(paths::effective_minecraft_root(
        settings.game_directory.as_deref(),
    ))
}

fn backup_directory() -> PathBuf {
    paths::default_launcher_root().join("backups")
}

fn prepare_profile(id: &str, root: &Path, require_complete: bool) -> Result<Profile, String> {
    let profile = profile::get_profile(id).map_err(|err| err.to_string())?;
    if profile.source == ProfileSource::Train
        && profile
            .game_dir
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
    {
        return Err(format!(
            "「{}」の専用ゲームフォルダーが未設定です。プロファイル画面で一度編集・保存してから、設定の取り込みや復元を行ってください。",
            profile.name
        ));
    }
    if require_complete {
        backup::ensure_restore_complete(&profile.effective_game_dir(root))
            .map_err(|err| err.to_string())?;
    }
    Ok(profile)
}

fn descriptor(profile: &Profile, root: &Path) -> Result<BackupProfile, CoreError> {
    let minecraft_version = game_settings::minecraft_version(&profile.minecraft_version, root)?;
    let mod_loader = match &profile.mod_loader {
        Some(loader) => Some(loader.trim().to_ascii_lowercase()),
        None if profile.minecraft_version.trim() != minecraft_version => {
            let path =
                paths::LauncherPaths::new(root).version_json_path(profile.minecraft_version.trim());
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
            Some(inherited_loader(&value, &profile.minecraft_version))
        }
        None => None,
    };
    Ok(BackupProfile {
        id: profile.id.clone(),
        name: profile.name.clone(),
        minecraft_version,
        mod_loader,
        managed_mod_filenames: profile.managed_mod_filenames.clone(),
        managed_resource_pack_filenames: profile.managed_resource_pack_filenames.clone(),
        enabled_resource_packs: profile.enabled_resource_packs.clone(),
        last_server_address: profile.last_server_address.clone(),
    })
}

fn inherited_loader(value: &serde_json::Value, id: &str) -> String {
    let main_class = value
        .get("mainClass")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let libraries: Vec<&str> = value
        .get("libraries")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("name").and_then(serde_json::Value::as_str))
        .collect();
    if main_class.contains("fabricmc.") || id.starts_with("fabric-loader-") {
        "fabric".to_string()
    } else if main_class.contains("quiltmc.") || id.starts_with("quilt-loader-") {
        "quilt".to_string()
    } else if main_class.contains("neoforged.")
        || id.starts_with("neoforge-")
        || libraries
            .iter()
            .any(|name| name.starts_with("net.neoforged:"))
    {
        "neoforge".to_string()
    } else if main_class.contains("minecraftforge.")
        || id.contains("-forge-")
        || libraries
            .iter()
            .any(|name| name.starts_with("net.minecraftforge:"))
    {
        "forge".to_string()
    } else {
        format!("custom:{id}")
    }
}

#[derive(Serialize)]
pub struct SettingsSource {
    id: String,
    name: String,
    minecraft_version: String,
    mod_loader: Option<String>,
    game_dir: String,
}

#[derive(Serialize)]
pub struct SettingsSourceList {
    minecraft_version: String,
    sources: Vec<SettingsSource>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
pub struct SettingsImportResult {
    imported_settings: usize,
    safety_backup: Option<BackupInfo>,
}

#[tauri::command]
pub fn get_game_activity(app_handle: AppHandle) -> Result<ActivityStatus, String> {
    app_handle.state::<GameActivity>().status()
}

#[tauri::command]
pub async fn list_settings_sources(
    target_profile_id: String,
) -> Result<SettingsSourceList, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = shared_root()?;
        let target = prepare_profile(&target_profile_id, &root, true)?;
        let target_version = game_settings::minecraft_version(&target.minecraft_version, &root)
            .map_err(|err| err.to_string())?;
        let target_directory = target.effective_game_dir(&root);
        let target_options = match game_settings::read_options(&target_directory) {
            Ok(text) => text,
            Err(CoreError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(err.to_string()),
        };
        let mut result = SettingsSourceList {
            minecraft_version: target_version.clone(),
            sources: Vec::new(),
            warnings: Vec::new(),
        };
        for source in profile::list_profiles().map_err(|err| err.to_string())? {
            if source.id == target_profile_id {
                continue;
            }
            let directory = source.effective_game_dir(&root);
            let options = match game_settings::read_options(&directory) {
                Ok(text) => text,
                Err(CoreError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    result.warnings.push(format!("{}: {err}", source.name));
                    continue;
                }
            };
            let source_version =
                match game_settings::minecraft_version(&source.minecraft_version, &root) {
                    Ok(version) => version,
                    Err(err) => {
                        result.warnings.push(format!("{}: {err}", source.name));
                        continue;
                    }
                };
            if source_version != target_version {
                continue;
            }
            if let (Ok(source_dir), Ok(target_dir)) = (
                std::fs::canonicalize(&directory),
                std::fs::canonicalize(&target_directory),
            ) {
                if source_dir == target_dir {
                    continue;
                }
            }
            if let Err(err) = backup::ensure_restore_complete(&directory)
                .and_then(|_| game_settings::merge_options(&options, &target_options).map(|_| ()))
            {
                result.warnings.push(format!("{}: {err}", source.name));
                continue;
            }
            let mod_loader = match descriptor(&source, &root) {
                Ok(info) => info.mod_loader,
                Err(err) => {
                    result.warnings.push(format!("{}: {err}", source.name));
                    continue;
                }
            };
            result.sources.push(SettingsSource {
                id: source.id,
                name: source.name,
                minecraft_version: source_version,
                mod_loader,
                game_dir: directory.display().to_string(),
            });
        }
        Ok(result)
    })
    .await
    .map_err(|err| format!("取り込み元の検索に失敗しました: {err}"))?
}

#[tauri::command]
pub async fn import_profile_settings(
    app_handle: AppHandle,
    source_profile_id: String,
    target_profile_id: String,
) -> Result<SettingsImportResult, String> {
    let activity = app_handle.state::<GameActivity>().begin_file_operation()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        if source_profile_id == target_profile_id {
            return Err("取り込み元と移行先は別のプロファイルを選択してください".into());
        }
        let root = shared_root()?;
        let source = profile::get_profile(&source_profile_id).map_err(|err| err.to_string())?;
        let target = prepare_profile(&target_profile_id, &root, true)?;
        let source_version = game_settings::minecraft_version(&source.minecraft_version, &root)
            .map_err(|err| err.to_string())?;
        let target_info = descriptor(&target, &root).map_err(|err| err.to_string())?;
        if source_version != target_info.minecraft_version {
            return Err("同じMinecraftバージョンのプロファイルからのみ取り込めます".into());
        }
        let source_dir = source.effective_game_dir(&root);
        let target_dir = target.effective_game_dir(&root);
        backup::ensure_restore_complete(&source_dir).map_err(|err| err.to_string())?;
        let source_text =
            game_settings::read_options(&source_dir).map_err(|err| err.to_string())?;
        let target_text = match game_settings::read_options(&target_dir) {
            Ok(text) => text,
            Err(CoreError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(err) => return Err(err.to_string()),
        };
        game_settings::merge_options(&source_text, &target_text).map_err(|err| err.to_string())?;
        let target_dir =
            game_settings::validate_destination(&target_dir).map_err(|err| err.to_string())?;
        if std::fs::canonicalize(&source_dir).map_err(|err| err.to_string())? == target_dir {
            return Err("同じゲームフォルダーへの取り込みはできません".into());
        }
        let safety_backup = if game_settings::has_importable_settings(&target_text)
            .map_err(|err| err.to_string())?
        {
            Some(
                backup::create_backup(
                    &backup_directory(),
                    &target_dir,
                    &target_info,
                    BackupScope::Settings,
                    true,
                )
                .map_err(|err| err.to_string())?,
            )
        } else {
            None
        };
        let imported_settings = game_settings::import_options(&source_dir, &target_dir)
            .map_err(|err| err.to_string())?;
        Ok(SettingsImportResult {
            imported_settings,
            safety_backup,
        })
    })
    .await
    .map_err(|err| format!("設定の取り込み処理に失敗しました: {err}"))?
}

#[tauri::command]
pub async fn list_profile_backups() -> Result<BackupList, String> {
    tauri::async_runtime::spawn_blocking(|| {
        backup::list_backups(&backup_directory()).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("バックアップの一覧を取得できません: {err}"))?
}

#[tauri::command]
pub async fn create_profile_backup(
    app_handle: AppHandle,
    profile_id: String,
    scope: BackupScope,
) -> Result<BackupInfo, String> {
    let activity = app_handle.state::<GameActivity>().begin_file_operation()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let root = shared_root()?;
        let profile = profile::get_profile(&profile_id).map_err(|err| err.to_string())?;
        backup::ensure_restore_complete(&profile.effective_game_dir(&root))
            .map_err(|err| err.to_string())?;
        let descriptor = descriptor(&profile, &root).map_err(|err| err.to_string())?;
        backup::create_backup(
            &backup_directory(),
            &profile.effective_game_dir(&root),
            &descriptor,
            scope,
            false,
        )
        .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("バックアップ処理に失敗しました: {err}"))?
}

#[tauri::command]
pub async fn restore_profile_backup(
    app_handle: AppHandle,
    backup_id: String,
    profile_id: String,
) -> Result<RestoreResult, String> {
    let activity = app_handle.state::<GameActivity>().begin_file_operation()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _activity = activity;
        let root = shared_root()?;
        let profile = prepare_profile(&profile_id, &root, false)?;
        let descriptor = descriptor(&profile, &root).map_err(|err| err.to_string())?;
        backup::restore_backup(
            &backup_directory(),
            &backup_id,
            &profile.effective_game_dir(&root),
            &descriptor,
            |restored| {
                if restored.scope == BackupScope::Full {
                    let mut updated = profile.clone();
                    updated.managed_mod_filenames = restored.profile.managed_mod_filenames.clone();
                    updated.managed_resource_pack_filenames =
                        restored.profile.managed_resource_pack_filenames.clone();
                    updated.enabled_resource_packs =
                        restored.profile.enabled_resource_packs.clone();
                    updated.last_server_address = restored.profile.last_server_address.clone();
                    profile::save_game_state(&updated)?;
                }
                Ok(())
            },
        )
        .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("バックアップの復元処理に失敗しました: {err}"))?
}

#[tauri::command]
pub fn open_backup_directory(app_handle: AppHandle) -> Result<(), String> {
    let directory = backup_directory();
    std::fs::create_dir_all(&directory).map_err(|err| err.to_string())?;
    app_handle
        .opener()
        .open_path(directory.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_inherited_loader_families_for_full_restore_compatibility() {
        assert_eq!(
            inherited_loader(
                &serde_json::json!({"mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient"}),
                "custom"
            ),
            "fabric"
        );
        assert_eq!(
            inherited_loader(
                &serde_json::json!({"libraries":[{"name":"net.neoforged:neoforge:21.1"}]}),
                "custom"
            ),
            "neoforge"
        );
        assert_eq!(
            inherited_loader(
                &serde_json::json!({"libraries":[{"name":"net.minecraftforge:forge:1.20.1"}]}),
                "custom"
            ),
            "forge"
        );
        assert_eq!(
            inherited_loader(&serde_json::json!({}), "unknown"),
            "custom:unknown"
        );
    }
}

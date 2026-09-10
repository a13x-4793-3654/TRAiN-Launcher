//! 公式Minecraft Launcherの `launcher_profiles.json` との相互運用。
//!
//! `.minecraft` フォルダを公式ランチャーと共有する設計([`crate::paths::default_minecraft_root`])
//! に伴い、起動プロファイルもTRAiN Launcher・公式ランチャーの双方で表示・編集できるようにする。
//!
//! `launcher_profiles.json` のフォーマットはMojang非公開だが、コミュニティにより広く
//! リバースエンジニアリングされている(参考: <https://minecraft.wiki/w/Launcher_profiles.json>)。
//! 本モジュールはそのファイルのうち `profiles` セクションのみを読み書きし、
//! `authenticationDatabase`(ログインセッション情報)等、他のセクションやTRAiN Launcherが
//! 関知しない未知のプロファイルフィールドは一切変更せずそのまま保持して書き戻す
//! (`serde_json::Value` として扱うことで、TRAiN Launcherが解釈しないフィールドを破壊しない)。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::CoreError;

fn launcher_profiles_path(minecraft_root: &Path) -> PathBuf {
    minecraft_root.join("launcher_profiles.json")
}

/// `launcher_profiles.json` の `profiles.<id>` エントリのうち、TRAiN Launcherが解釈する範囲。
#[derive(Debug, Clone)]
pub struct OfficialProfile {
    pub id: String,
    pub name: String,
    pub last_version_id: Option<String>,
    pub java_dir: Option<String>,
    pub java_args: Option<String>,
    /// このプロファイル専用のゲームディレクトリ上書き(公式ランチャー側の `gameDir`)。
    /// 未指定の場合は公式ランチャーの既定の `.minecraft` を使う。
    pub game_dir: Option<String>,
}

/// `launcher_profiles.json` の `profiles` セクションを読み込む。
///
/// ファイルが存在しない場合(公式ランチャーを一度も起動していない)は空一覧を返す。
pub fn read_all(minecraft_root: &Path) -> Result<Vec<OfficialProfile>, CoreError> {
    let path = launcher_profiles_path(minecraft_root);
    let root = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };

    let Some(profiles) = root.get("profiles").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };

    let mut result = Vec::with_capacity(profiles.len());
    for (id, value) in profiles {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_string();
        let last_version_id = value
            .get("lastVersionId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let java_dir = value
            .get("javaDir")
            .and_then(Value::as_str)
            .map(str::to_string);
        let java_args = value
            .get("javaArgs")
            .and_then(Value::as_str)
            .map(str::to_string);
        let game_dir = value
            .get("gameDir")
            .and_then(Value::as_str)
            .map(str::to_string);
        result.push(OfficialProfile {
            id: id.clone(),
            name,
            last_version_id,
            java_dir,
            java_args,
            game_dir,
        });
    }
    Ok(result)
}

/// TRAiN Launcherのプロファイルを `launcher_profiles.json` の `profiles.<id>` に反映する
/// (公式ランチャー側の起動構成一覧にも同じプロファイルとして表示・起動できるようにする)。
///
/// ファイルが存在しない場合は最小限の構造で新規作成する。既存の他プロファイルや
/// `authenticationDatabase` 等は保持したまま、該当IDのエントリのみ追加・更新する。
pub fn upsert_profile(minecraft_root: &Path, profile: &crate::profile::Profile) -> Result<(), CoreError> {
    let path = launcher_profiles_path(minecraft_root);
    let mut root = read_root_or_default(&path)?;

    let profiles = root
        .get_mut("profiles")
        .and_then(Value::as_object_mut)
        .expect("read_root_or_default always ensures a `profiles` object");

    // `created` は既存エントリがあれば引き継ぎ、無ければ現在時刻を使う。
    let created = profiles
        .get(&profile.id)
        .and_then(|entry| entry.get("created"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(now_iso8601);

    let mut entry = json!({
        "name": profile.name,
        "type": "custom",
        "created": created,
        "lastUsed": now_iso8601(),
        "lastVersionId": profile.minecraft_version,
    });
    if let Some(java_path) = &profile.java_path {
        entry["javaDir"] = json!(java_path);
    }
    if let Some(max_memory_mb) = profile.max_memory_mb {
        entry["javaArgs"] = json!(format!("-Xmx{max_memory_mb}M -Xms{max_memory_mb}M"));
    }
    if let Some(game_dir) = &profile.game_dir {
        entry["gameDir"] = json!(game_dir);
    }

    profiles.insert(profile.id.clone(), entry);
    write_root(&path, &root)
}

/// `launcher_profiles.json` から指定IDのプロファイルを削除する。
///
/// ファイル自体が存在しない場合、またはそのIDのエントリが存在しない場合は何もせず
/// `false` を返す(呼び出し側でTRAiN側にも存在しないIDだった場合の判定に使う)。
pub fn remove_profile(minecraft_root: &Path, id: &str) -> Result<bool, CoreError> {
    let path = launcher_profiles_path(minecraft_root);
    if !path.exists() {
        return Ok(false);
    }
    let mut root = read_root_or_default(&path)?;
    let removed = root
        .get_mut("profiles")
        .and_then(Value::as_object_mut)
        .map(|profiles| profiles.remove(id).is_some())
        .unwrap_or(false);
    if removed {
        write_root(&path, &root)?;
    }
    Ok(removed)
}

fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn default_settings() -> Value {
    json!({
        "enableAdvanced": false,
        "enableSnapshots": false,
        "keepLauncherOpen": false,
        "profileSorting": "byLastPlayed",
        "showGameLog": false,
        "showMenu": false,
    })
}

/// `launcher_profiles.json` を読み込み、`profiles`/`settings`/`version` トップレベルキーが
/// 必ず存在する状態に補正して返す。ファイルが存在しない場合は最小限の構造を新規に返す。
fn read_root_or_default(path: &Path) -> Result<Value, CoreError> {
    let mut root = match std::fs::read(path) {
        Ok(bytes) => {
            let value = serde_json::from_slice::<Value>(&bytes)?;
            if value.is_object() {
                value
            } else {
                json!({})
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(err) => return Err(err.into()),
    };

    if root.get("profiles").and_then(Value::as_object).is_none() {
        root["profiles"] = json!({});
    }
    if root.get("settings").is_none() {
        root["settings"] = default_settings();
    }
    if root.get("version").is_none() {
        root["version"] = json!(3);
    }
    Ok(root)
}

fn write_root(path: &Path, root: &Value) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(root)?)?;
    Ok(())
}

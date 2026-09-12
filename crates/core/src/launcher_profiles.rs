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

/// 公式ランチャーが「最新リリース」「最新スナップショット」を自動追従するために内部で
/// 使う組み込みプロファイルの`type`値に対応する、人間が読める既定名を返す。
/// 該当しない`type`(通常のカスタムプロファイル等)の場合は`None`を返す。
fn default_name_for_builtin_type(profile_type: Option<&str>) -> Option<&'static str> {
    match profile_type {
        Some("latest-release") => Some("最新リリース(自動追従)"),
        Some("latest-snapshot") => Some("最新スナップショット(自動追従)"),
        _ => None,
    }
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
        // 公式ランチャーが自動生成する「最新リリース」「最新スナップショット」プロファイルは
        // `"name": ""`(キー自体は存在するが空文字列)を持つ。`.and_then(Value::as_str)`は
        // キーが存在する限り`Some("")`を返すため、`.unwrap_or(id)`ではこのケースを
        // 補完できず、名前が空のまま(=UI上でオプションが空白になる)になってしまっていた。
        // キーが存在しない場合・空文字列の場合の両方をカバーし、既知の組み込み種別には
        // 分かりやすい既定名を、それ以外はIDをフォールバックとして使う。
        let raw_name = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let profile_type = value.get("type").and_then(Value::as_str);
        let name = raw_name
            .or_else(|| default_name_for_builtin_type(profile_type))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_profiles_json(minecraft_root: &Path, profiles: Value) {
        std::fs::create_dir_all(minecraft_root).unwrap();
        let root = json!({ "profiles": profiles });
        std::fs::write(
            launcher_profiles_path(minecraft_root),
            serde_json::to_vec_pretty(&root).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn read_all_falls_back_to_id_when_name_key_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        write_profiles_json(
            dir.path(),
            json!({ "some-id": { "type": "custom" } }),
        );

        let profiles = read_all(dir.path()).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "some-id");
    }

    #[test]
    fn read_all_falls_back_to_id_when_name_is_blank_string() {
        // 公式ランチャーが「(Default)」以外の組み込みプロファイルで生成する
        // `"name": ""` を再現する。以前は空文字列のままになっていた。
        let dir = tempfile::tempdir().unwrap();
        write_profiles_json(
            dir.path(),
            json!({ "abc123": { "name": "  ", "type": "custom" } }),
        );

        let profiles = read_all(dir.path()).unwrap();
        assert_eq!(profiles[0].name, "abc123");
    }

    #[test]
    fn read_all_uses_friendly_name_for_builtin_latest_release() {
        let dir = tempfile::tempdir().unwrap();
        write_profiles_json(
            dir.path(),
            json!({
                "5ae6af95b2032ffb30272eef038adc34": {
                    "name": "",
                    "type": "latest-release",
                    "lastVersionId": "latest-release"
                }
            }),
        );

        let profiles = read_all(dir.path()).unwrap();
        assert_eq!(profiles[0].name, "最新リリース(自動追従)");
    }

    #[test]
    fn read_all_uses_friendly_name_for_builtin_latest_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        write_profiles_json(
            dir.path(),
            json!({
                "0387150b193857878b3f797ad63b2030": {
                    "name": "",
                    "type": "latest-snapshot",
                    "lastVersionId": "latest-snapshot"
                }
            }),
        );

        let profiles = read_all(dir.path()).unwrap();
        assert_eq!(profiles[0].name, "最新スナップショット(自動追従)");
    }

    #[test]
    fn read_all_keeps_custom_name_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        write_profiles_json(
            dir.path(),
            json!({ "custom-id": { "name": "碓氷鯖 (本番)", "type": "custom" } }),
        );

        let profiles = read_all(dir.path()).unwrap();
        assert_eq!(profiles[0].name, "碓氷鯖 (本番)");
    }
}

//! TRAiN API のデータモデル。
//!
//! TRAiNバックエンド側 `docs/LAUNCHER-API.md` のレスポンス形式と一致させている。

use serde::{Deserialize, Serialize};

/// ユーザーが所属しているTRAiN管理サーバー。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberServer {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_server_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<MemberServerStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberServerStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<MemberServerPalette>,
}

/// Display metadata only; the frontend validates colors before using them as CSS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberServerPalette {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreground: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<String>,
}

/// サーバーごとの設定情報(接続先・Mod構成・リソースパックなど)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub server_id: String,
    pub address: String,
    pub minecraft_version: String,
    pub mod_loader: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fabric_loader_version: Option<String>,
    pub mod_urls: Vec<String>,
    pub resource_pack_urls: Vec<String>,
    /// このBearerトークンの持ち主(Discordアカウント)が、このサーバーのギルドで既に
    /// Discord↔Minecraftアカウントの紐づけを完了しているか。
    ///
    /// TRAiN側の対応前は未実装のため欠落しうる(その場合 `None`)。呼び出し側は
    /// `None`/`Some(true)` の場合は初回参加の案内を出さない(誤って毎回表示しない
    /// ようにするため、判定できない場合は「紐づけ済み」寄りに倒す)。
    #[serde(default)]
    pub linked: Option<bool>,
    /// このサーバーが「試験モード」中で、かつこのBearerトークンの持ち主がそのDiscordギルドで
    /// Administrator権限を持っているため、`mod_urls`/`resource_pack_urls`(および
    /// `minecraft_version`/`mod_loader`)が本番構成ではなく試験用構成になっているか。
    ///
    /// `true` の場合、ランチャーは起動前に試験モードである旨をモーダルで通知すること。
    /// TRAiN側の対応前は未実装のため欠落しうる(その場合 `None`、本番構成として扱う)。
    #[serde(default)]
    pub test_mode: Option<bool>,
}

/// [`TrainApiClient::link_account`] のリクエスト本体。
///
/// ランチャーが既に確認済みの、Microsoft/Xbox認証済みMinecraftアカウント情報を渡す
/// (ゲーム内で表示される認証コード + Discordの `/link` コマンドという通常の手順を
/// 経由せず、ランチャー自身が本人性を証明した上で直接紐づける)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkAccountRequest {
    pub mc_uuid: String,
    pub mc_name: String,
}

/// お知らせ1件(グローバル/サーバー個別で共通の形式)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Announcement {
    pub id: String,
    pub title: String,
    pub body: String,
    /// `"info"` | `"warning"` | `"critical"`。
    pub severity: String,
    /// ISO8601 (UTC)。
    pub published_at: String,
}

/// [`TrainApiClient::submit_crash_report`] のリクエスト本体。
///
/// TRAiN側の `POST /api/servers/{server_id}/crash-reports` の契約
/// (`docs/LAUNCHER-API.md` 7章)に合わせる。送る前に必ず本人の同意を得ること。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReportSubmission {
    /// `"crash_report"`(`crash-reports/crash-*-client.txt`)または
    /// `"jvm_crash"`(`hs_err_pid*.log`)。
    pub kind: String,
    pub file_name: String,
    /// ISO8601 (UTC)。
    pub crashed_at: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minecraft_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_loader: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launcher_version: Option<String>,
}

/// [`TrainApiClient::submit_crash_report`] の応答(登録されたレポートのID)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReportReceipt {
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_config_preserves_fabric_pin_and_accepts_legacy_field_absence() {
        let mut json = serde_json::json!({
            "server_id": "workspace", "address": "workspace.example.com:25565",
            "minecraft_version": "1.21.1", "mod_loader": "fabric",
            "mod_urls": ["https://example.com/workspace.jar"], "resource_pack_urls": []
        });
        let legacy: ServerConfig = serde_json::from_value(json.clone()).unwrap();
        assert!(legacy.fabric_loader_version.is_none());
        json["fabric_loader_version"] = "0.19.5".into();
        let current: ServerConfig = serde_json::from_value(json).unwrap();
        let restored: ServerConfig =
            serde_json::from_value(serde_json::to_value(current).unwrap()).unwrap();
        assert_eq!(restored.fabric_loader_version.as_deref(), Some("0.19.5"));
        assert_eq!(restored.server_id, "workspace");
        assert_eq!(restored.mod_urls, ["https://example.com/workspace.jar"]);
    }

    #[test]
    fn member_servers_preserve_api_metadata_through_ipc_and_json_roundtrip() {
        let json: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/member-servers.json")).unwrap();
        let servers: Vec<MemberServer> = serde_json::from_value(json.clone()).unwrap();
        assert!(servers[0].environment.is_none());
        assert!(servers[0].style.is_none());
        assert_eq!(serde_json::to_value(&servers).unwrap(), json);
        let restored: Vec<MemberServer> =
            serde_json::from_str(&serde_json::to_string(&servers).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), json);
    }

    #[test]
    fn optional_metadata_accepts_old_missing_null_partial_and_unknown_values() {
        for json in [
            r#"{"id":"old","name":"Old"}"#,
            r#"{"id":"null","name":"Null","environment":null,"style":null}"#,
            r##"{"id":"partial","name":"Partial","environment":"workspace","style":{"palette":{"background":"#352613"}}}"##,
            r#"{"id":"future","name":"Future","environment":"future","style":{"badge":"Other","palette":null}}"#,
        ] {
            let server: MemberServer = serde_json::from_str(json).unwrap();
            let restored: MemberServer =
                serde_json::from_value(serde_json::to_value(server).unwrap()).unwrap();
            assert!(!restored.id.is_empty());
        }
    }
}

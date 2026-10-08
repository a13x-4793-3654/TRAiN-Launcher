//! TRAiN API のデータモデル。
//!
//! TRAiNバックエンド側 `docs/LAUNCHER-API.md` のレスポンス形式と一致させている。

use serde::{Deserialize, Serialize};

/// ユーザーが所属しているTRAiN管理サーバー。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberServer {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub parent_server_id: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub style: Option<WorkspaceStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStyle {
    pub badge: String,
    pub palette: WorkspacePalette,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacePalette {
    pub background: String,
    pub foreground: String,
    pub border: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpiredWorkspaces {
    pub server_ids: Vec<String>,
}

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn legacy_and_workspace_membership_deserialize() {
        let production: MemberServer =
            serde_json::from_str(r#"{"id":"prod","name":"Production"}"#).unwrap();
        assert!(production.environment.is_none());
        let workspace: MemberServer = serde_json::from_str(
            r##"{"id":"ws","name":"Test","environment":"workspace","parent_server_id":"prod","expires_at":"2026-10-01T00:00:00Z","style":{"badge":"検証","palette":{"background":"#112233","foreground":"#ffffff","border":"#334455"}}}"##,
        ).unwrap();
        assert_eq!(workspace.parent_server_id.as_deref(), Some("prod"));
        assert_eq!(workspace.style.unwrap().badge, "検証");
        let expired: ExpiredWorkspaces =
            serde_json::from_str(r#"{"server_ids":["ws"]}"#).unwrap();
        assert_eq!(expired.server_ids, vec!["ws"]);
    }
}

/// サーバーごとの設定情報(接続先・Mod構成・リソースパックなど)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub server_id: String,
    pub address: String,
    pub minecraft_version: String,
    pub mod_loader: Option<String>,
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

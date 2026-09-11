//! TRAiN API のデータモデル。
//!
//! TRAiNバックエンド側 `docs/LAUNCHER-API.md` のレスポンス形式と一致させている。

use serde::{Deserialize, Serialize};

/// ユーザーが所属しているTRAiN管理サーバー。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberServer {
    pub id: String,
    pub name: String,
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

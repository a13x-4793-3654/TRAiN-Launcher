//! train-launcher-server-api
//!
//! TRAiN独自バックエンドAPIのクライアント。Discordサインイン後、ユーザーが所属している
//! サーバー一覧・各サーバーの設定(接続先・Mod構成・リソースパックなど)を取得するために使う。
//!
//! エンドポイント形式・認証方式・エラーコードはTRAiNバックエンド側の
//! `docs/LAUNCHER-API.md` で確定済み(`GET {base}/api/members/{discord_user_id}/servers` /
//! `GET {base}/api/servers/{server_id}/config`、`Authorization: Bearer <Discordアクセス
//! トークン>`)。[`create_client`] は環境変数 [`API_BASE_URL_ENV_VAR`]
//! (`TRAIN_LAUNCHER_API_BASE_URL`) が未設定の場合、ビルド時に埋め込まれた既定URL
//! ([`DEFAULT_API_BASE_URL`]、公式リリースビルドではGitHub Actionsのリポジトリ変数経由で
//! 本番TRAiN APIが設定される)へ接続する。既定URLも埋め込まれていないローカル開発ビルドでは
//! [`MockTrainApiClient`] にフォールバックする。モック実装を明示的に使いたい場合は、
//! この環境変数に `mock` を設定する。
//!
//! 追加エンドポイント: `POST {base}/api/servers/{server_id}/link`(初回参加時、ランチャーが
//! 既に確認済みのDiscord/Minecraftアカウント情報を使って紐づけを直接完了させる。
//! 詳細は [`LinkAccountRequest`] / [`TrainApiClient::link_account`] を参照。
//! TRAiN側で実装済み(`GET .../config` の `linked` フィールドと合わせて提供)。
//! 成功時のレスポンス本文(`{ "linked": true }`)は使用しない(ステータスコードのみで判定)。
//!
//! 追加エンドポイント: `GET {base}/api/announcements`(グローバルお知らせ、認証不要)/
//! `GET {base}/api/servers/{server_id}/announcements`(サーバー個別お知らせ、`.../config`
//! と同じ認証・非開示ポリシー)。詳細は [`Announcement`] /
//! [`TrainApiClient::get_global_announcements`] / [`TrainApiClient::get_server_announcements`]
//! を参照。

pub mod models;

use async_trait::async_trait;
use serde::Deserialize;

pub use models::{Announcement, LinkAccountRequest, MemberServer, ServerConfig};

/// train-launcher-server-api 全体で使用するエラー型。
///
/// バリアントのメッセージはそのままフロントエンドのエラー表示に使われるため
/// (`Result<_, String>` を返すTauriコマンドが`err.to_string()`するだけ)、
/// 日本語かつユーザーが次に何をすればよいか分かる文面にする。
#[derive(Debug, thiserror::Error)]
pub enum TrainApiError {
    #[error("not implemented yet: {0}")]
    NotImplemented(&'static str),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// `401 unauthorized`: トークンが無い、またはDiscordがトークンを認めなかった。
    #[error("Discordで再サインインしてください(認証が期限切れ、または無効です)")]
    Unauthorized,
    /// `403 forbidden`: トークンの持ち主とリクエスト対象のDiscordユーザーが一致しない。
    #[error("このDiscordアカウントでは所属サーバー情報を取得できません")]
    Forbidden,
    /// `400`: パスパラメータの形式が不正(Discordスノーフレーク/UUIDでない等)。
    #[error("リクエスト内容が正しくありません")]
    InvalidRequest,
    /// `404 not_found`: サーバーが存在しない、または本人がそのギルドのメンバーでない。
    #[error("サーバーが見つからないか、参加資格がありません")]
    NotFound,
    /// `404 not_configured`: サーバーは登録されているが、`minecraft-mods manifest`
    /// (または`apply`)がTRAiN Link経由で一度も送られておらず、接続先/Mod構成が無い。
    #[error(
        "このサーバーはまだ配布設定(接続先・Mod構成)が登録されていません。サーバー管理者に確認してください"
    )]
    NotConfigured,
    /// `405 method_not_allowed`: 想定外のHTTPメソッド(通常は発生しない)。
    #[error("このAPIリクエストは許可されていません")]
    MethodNotAllowed,
    /// 409 `already_linked`(`link_account`のみ): このDiscordアカウントは、このサーバーの
    /// Discordサーバー(ギルド)内で既に別のMinecraftアカウントと紐づけ済み。
    #[error(
        "このDiscordアカウントは既に別のMinecraftアカウントと紐づけられています。心当たりが\
         ない場合はサーバー管理者に確認してください"
    )]
    AlreadyLinked,
    /// 409 `uuid_already_linked`(`link_account`のみ): このMinecraftアカウント(UUID)は、
    /// このサーバーのDiscordサーバー(ギルド)内で既に別のDiscordアカウントと紐づけ済み。
    #[error(
        "このMinecraftアカウントは既に別のDiscordアカウントと紐づけられています。心当たりが\
         ない場合はサーバー管理者に確認してください"
    )]
    UuidAlreadyLinked,
    /// 上記に当てはまらない未知のエラーコード。TRAiN側の仕様変更などを検知できるよう、
    /// status/codeをそのまま表示する。
    #[error("APIエラーが発生しました(status: {status}, code: {code})")]
    Api { status: u16, code: String },
}

/// TRAiN APIのエラーレスポンス本文(`{ "error": "..." }`)。
#[derive(Debug, Deserialize)]
struct ErrorBody {
    error: String,
}

/// 失敗レスポンスを [`TrainApiError`] へ変換する。
///
/// `docs/LAUNCHER-API.md` に列挙されたエラーコードに基づいてマッピングする。
/// 本文が読めない/期待した形式でない場合でも、statusから可能な範囲で判定する。
async fn map_error_response(response: reqwest::Response) -> TrainApiError {
    let status = response.status().as_u16();
    let code = response
        .json::<ErrorBody>()
        .await
        .map(|body| body.error)
        .unwrap_or_default();
    match (status, code.as_str()) {
        (401, _) => TrainApiError::Unauthorized,
        (403, _) => TrainApiError::Forbidden,
        (404, "not_configured") => TrainApiError::NotConfigured,
        (404, _) => TrainApiError::NotFound,
        (405, _) => TrainApiError::MethodNotAllowed,
        (409, "already_linked") => TrainApiError::AlreadyLinked,
        (409, "uuid_already_linked") => TrainApiError::UuidAlreadyLinked,
        (400, _) => TrainApiError::InvalidRequest,
        _ => TrainApiError::Api {
            status,
            code,
        },
    }
}

/// TRAiNバックエンドAPIクライアントの振る舞いを定義するトレイト。
#[async_trait]
pub trait TrainApiClient: Send + Sync {
    /// Discordサインイン後、ユーザーが所属しているTRAiN管理サーバー一覧を取得する。
    async fn get_member_servers(
        &self,
        discord_user_id: &str,
    ) -> Result<Vec<MemberServer>, TrainApiError>;

    /// 指定サーバーの設定(接続先・Mod構成・リソースパックなど)を取得する。
    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError>;

    /// ランチャーが既に確認済みのMinecraftアカウント情報(Microsoft/Xbox認証済みの
    /// UUID・プレイヤー名)を使い、Discordアカウントとの紐づけを直接完了させる。
    ///
    /// 通常の手順(Minecraft参加時にゲーム内へ表示される認証コードをDiscordの
    /// `/link` コマンドへ入力する)を経由しない代替経路。呼び出し元は事前にユーザーへ
    /// 紐づけの内容(何と何がどう結び付くか)を提示し、同意を得た上で呼び出すこと。
    async fn link_account(
        &self,
        server_id: &str,
        request: &LinkAccountRequest,
    ) -> Result<(), TrainApiError>;

    /// 現在有効なグローバル(ランチャー全体共通)のお知らせを新しい順に取得する。
    /// 認証不要(未サインインでも呼べる)。
    async fn get_global_announcements(&self) -> Result<Vec<Announcement>, TrainApiError>;

    /// 指定サーバーのお知らせを新しい順に取得する。認証・非開示ポリシーは
    /// [`TrainApiClient::get_server_config`] と同じ(所属メンバーのみ閲覧可能)。
    async fn get_server_announcements(
        &self,
        server_id: &str,
    ) -> Result<Vec<Announcement>, TrainApiError>;
}

/// 開発・テスト用のモック実装。常にダミーデータを返す。
pub struct MockTrainApiClient;

#[async_trait]
impl TrainApiClient for MockTrainApiClient {
    async fn get_member_servers(
        &self,
        _discord_user_id: &str,
    ) -> Result<Vec<MemberServer>, TrainApiError> {
        Ok(vec![MemberServer {
            id: "mock-server-1".to_string(),
            name: "TRAiN Mock Server".to_string(),
        }])
    }

    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError> {
        Ok(ServerConfig {
            server_id: server_id.to_string(),
            address: "play.example.com:25565".to_string(),
            minecraft_version: "1.21".to_string(),
            mod_loader: Some("fabric".to_string()),
            mod_urls: vec![],
            resource_pack_urls: vec![],
            // モック環境では実サーバーが存在しないため紐づけ状態を判定できない。
            // 初回参加時の注意事項モーダルをUI上で確認できるよう、常に未紐づけ扱いにする。
            linked: Some(false),
            // 同様に、試験モード通知モーダルをUI上で確認できるよう、常に試験モード扱いにする。
            test_mode: Some(true),
        })
    }

    async fn link_account(
        &self,
        _server_id: &str,
        _request: &LinkAccountRequest,
    ) -> Result<(), TrainApiError> {
        // モック環境では常に成功させる(UI側の同意〜完了までの流れを確認できるようにする)。
        Ok(())
    }

    async fn get_global_announcements(&self) -> Result<Vec<Announcement>, TrainApiError> {
        Ok(vec![Announcement {
            id: "mock-announcement-global-1".to_string(),
            title: "ランチャーへようこそ".to_string(),
            body: "TRAiN Launcherのモック環境です。実際のお知らせはTRAiN API接続後に表示されます。"
                .to_string(),
            severity: "info".to_string(),
            published_at: "2026-01-01T00:00:00.000Z".to_string(),
        }])
    }

    async fn get_server_announcements(
        &self,
        _server_id: &str,
    ) -> Result<Vec<Announcement>, TrainApiError> {
        Ok(vec![Announcement {
            id: "mock-announcement-server-1".to_string(),
            title: "メンテナンス予定".to_string(),
            body: "このサーバーは近日中にメンテナンスを予定しています(モックデータ)。"
                .to_string(),
            severity: "warning".to_string(),
            published_at: "2026-01-02T00:00:00.000Z".to_string(),
        }])
    }
}

/// TRAiN APIのベースURLを指定する環境変数名。
///
/// 設定する場合はスキーム込みで指定する(例: `https://api.train.example.com`)。
/// 未設定(または空文字列)の場合は [`DEFAULT_API_BASE_URL`] が使われる。
/// 開発時にモック実装を強制したい場合は、この環境変数に `mock`(大文字小文字は区別しない)
/// を設定することで [`create_client`] は [`MockTrainApiClient`] を返す。
pub const API_BASE_URL_ENV_VAR: &str = "TRAIN_LAUNCHER_API_BASE_URL";

/// [`API_BASE_URL_ENV_VAR`](実行時の環境変数)が未設定の場合に使われる既定のベースURL。
///
/// ソースコードに直接ハードコードせず、**ビルド時**の環境変数
/// `TRAIN_LAUNCHER_DEFAULT_API_BASE_URL` から [`option_env!`] で埋め込む。
/// 公式リリースビルド(`.github/workflows/release.yml`)は、GitHub Actionsのリポジトリ
/// 変数(Settings > Secrets and variables > Actions > Variables、`TRAIN_LAUNCHER_API_BASE_URL`)
/// からこの値を渡すことで、本番TRAiN APIのURLを組み込む。
/// ローカル開発でこのビルド時環境変数を設定しなかった場合は空文字列となり、
/// [`create_client`] は([`API_BASE_URL_ENV_VAR`] も未設定なら)モック実装にフォールバックする
/// (従来通りのローカル開発時の既定動作)。
pub const DEFAULT_API_BASE_URL: &str = match option_env!("TRAIN_LAUNCHER_DEFAULT_API_BASE_URL") {
    Some(value) => value,
    None => "",
};

/// TRAiNバックエンドAPIの実HTTPクライアント実装。
///
/// エンドポイント形式(`GET {base}/api/members/{discord_user_id}/servers` /
/// `GET {base}/api/servers/{server_id}/config`)・認証方式・エラーコードは
/// TRAiN側の`docs/LAUNCHER-API.md`で確定済み。TRAiN側は新規のOAuthを実装せず、
/// 渡されたDiscordアクセストークンをそのまま`GET https://discord.com/api/v10/users/@me`
/// へ投げて本人確認を行うため、ここでは受け取ったトークンをBearerとして渡すだけでよい。
pub struct HttpTrainApiClient {
    base_url: String,
    /// Discordアクセストークン。設定されていれば認証ヘッダー(Bearerスキーム)として
    /// 各リクエストに付与する(TRAiN側がDiscord APIへ問い合わせて本人確認を行う)。
    discord_access_token: Option<String>,
    client: reqwest::Client,
}

impl HttpTrainApiClient {
    pub fn new(base_url: impl Into<String>, discord_access_token: Option<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            discord_access_token,
            client: reqwest::Client::new(),
        }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        let request = self.client.get(format!("{}{path}", self.base_url));
        match &self.discord_access_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        let request = self.client.post(format!("{}{path}", self.base_url));
        match &self.discord_access_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }
}

#[async_trait]
impl TrainApiClient for HttpTrainApiClient {
    async fn get_member_servers(
        &self,
        discord_user_id: &str,
    ) -> Result<Vec<MemberServer>, TrainApiError> {
        let response = self
            .get(&format!("/api/members/{discord_user_id}/servers"))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json::<Vec<MemberServer>>().await?)
    }

    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError> {
        let response = self
            .get(&format!("/api/servers/{server_id}/config"))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json::<ServerConfig>().await?)
    }

    async fn link_account(
        &self,
        server_id: &str,
        request: &LinkAccountRequest,
    ) -> Result<(), TrainApiError> {
        let response = self
            .post(&format!("/api/servers/{server_id}/link"))
            .json(request)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(())
    }

    async fn get_global_announcements(&self) -> Result<Vec<Announcement>, TrainApiError> {
        let response = self.get("/api/announcements").send().await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json::<Vec<Announcement>>().await?)
    }

    async fn get_server_announcements(
        &self,
        server_id: &str,
    ) -> Result<Vec<Announcement>, TrainApiError> {
        let response = self
            .get(&format!("/api/servers/{server_id}/announcements"))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json::<Vec<Announcement>>().await?)
    }
}

/// 環境変数 [`API_BASE_URL_ENV_VAR`](実行時)と [`DEFAULT_API_BASE_URL`](ビルド時に
/// 埋め込まれた既定値)に応じて実クライアント/モックを切り替える。
///
/// - 実行時環境変数に `mock`(大文字小文字を区別しない)が設定されている場合は
///   [`MockTrainApiClient`]。
/// - それ以外の空でない値が設定されている場合は、その値をベースURLとした
///   [`HttpTrainApiClient`](ローカル開発時の接続先上書き用)。
/// - 実行時環境変数が未設定(または空文字列)の場合、[`DEFAULT_API_BASE_URL`] が空でなければ
///   それをベースURLとした [`HttpTrainApiClient`](公式リリースビルドの既定動作)、
///   空(ビルド時に埋め込まれていない、ローカル開発ビルドなど)なら [`MockTrainApiClient`]。
///
/// `discord_access_token` は [`HttpTrainApiClient`] 使用時、認証ヘッダーとして付与される
/// (詳細は [`HttpTrainApiClient`] のドキュメント参照)。
pub fn create_client(discord_access_token: Option<String>) -> Box<dyn TrainApiClient> {
    match std::env::var(API_BASE_URL_ENV_VAR) {
        Ok(value) if value.trim().eq_ignore_ascii_case("mock") => Box::new(MockTrainApiClient),
        Ok(base_url) if !base_url.trim().is_empty() => {
            Box::new(HttpTrainApiClient::new(base_url, discord_access_token))
        }
        _ if !DEFAULT_API_BASE_URL.is_empty() => Box::new(HttpTrainApiClient::new(
            DEFAULT_API_BASE_URL,
            discord_access_token,
        )),
        _ => Box::new(MockTrainApiClient),
    }
}

#[cfg(test)]
mod tests {
    /// ビルド時環境変数 `TRAIN_LAUNCHER_DEFAULT_API_BASE_URL` が [`option_env!`] 経由で
    /// [`super::DEFAULT_API_BASE_URL`] へ正しく埋め込まれることを確認する回帰テスト。
    /// 未設定でビルドした場合は空文字列になる(ローカル開発時の既定=モック実装)ことも検証する。
    #[test]
    fn default_api_base_url_reflects_build_time_env_var() {
        match option_env!("TRAIN_LAUNCHER_DEFAULT_API_BASE_URL") {
            Some(expected) => assert_eq!(super::DEFAULT_API_BASE_URL, expected),
            None => assert_eq!(super::DEFAULT_API_BASE_URL, ""),
        }
    }
}

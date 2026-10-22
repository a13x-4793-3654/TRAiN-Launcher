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
//! Discord認証に加え、Rust内で取得したMinecraftアクセストークンで所有権を証明する。
//! UUID・名前はバックエンドがMinecraft APIで照合し、確認済みの正規値だけを保存する。
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
pub mod fallback;

use async_trait::async_trait;
use serde::Deserialize;

pub use models::{
    Announcement, CrashReportReceipt, CrashReportSubmission, LinkAccountRequest, MemberServer,
    ServerConfig,
};

/// train-launcher-server-api 全体で使用するエラー型。
///
/// バリアントのメッセージはそのままフロントエンドのエラー表示に使われるため
/// (`Result<_, String>` を返すTauriコマンドが`err.to_string()`するだけ)、
/// 日本語かつユーザーが次に何をすればよいか分かる文面にする。
#[derive(Debug, thiserror::Error)]
pub enum TrainApiError {
    #[error("障害時接続は利用できません。管理者による有効化と対応Modの配布を確認してください")]
    FallbackUnavailable,
    #[error("Minecraft認証サービスに接続できません。紐づけ・事前登録は復旧後に行ってください")]
    MinecraftUnavailable,
    #[error("Minecraftアカウントの所有権を確認できませんでした。Microsoftで再サインインし、同じMinecraftアカウントでやり直してください")]
    MinecraftIdentityMismatch,
    #[error("Minecraftの所有権証明はHTTPS接続でのみ送信できます。TRAiN APIの接続先を確認してください(HTTPはローカル開発用のループバック接続のみ許可されます)")]
    InsecureCredentialTransport,
    #[error(
        "Minecraftアカウントの紐づけ・本人確認または参加資格を確認できませんでした。資格情報の期限切れも考えられます。期限切れの場合はMinecraft認証の復旧後に再登録してください"
    )]
    FallbackDenied,
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
    #[error("リクエスト内容が正しくありません。ランチャーを更新し、再サインインしてやり直してください")]
    InvalidRequest,
    /// `404 not_found`: サーバーが存在しない、または本人がそのギルドのメンバーでない。
    #[error("サーバーが見つからないか、参加資格がありません")]
    NotFound,
    /// `404 not_configured`: サーバーは登録されているが、`minecraft-mods manifest`
    /// (または`apply`)がTRAiN Link経由で一度も送られておらず、接続先/Mod構成が無い。
    #[error(
        "このサーバーの配布設定(接続先・Mod構成)は未登録または反映待ちです。Workspaceの操作・設定反映が完了してから再試行してください。サーバー管理者に確認してください"
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
    /// 403 `not_linked`(`submit_crash_report`のみ): このサーバーのギルドで、まだ
    /// Minecraftアカウントとの紐づけが済んでいない。
    #[error(
        "このサーバーではまだMinecraftアカウントの紐づけが済んでいないため、クラッシュレポートを\
         送れません。一度サーバーへ参加して紐づけを済ませてください"
    )]
    NotLinked,
    /// 422 `not_connected`(`submit_crash_report`のみ): クラッシュ時刻の前後に、
    /// そのサーバーへ参加していた記録がTRAiN側に無い。
    #[error(
        "クラッシュした時刻にこのサーバーへ接続していた記録が見つからないため、送信できませんでした"
    )]
    NotConnected,
    /// 413 `payload_too_large`(`submit_crash_report`のみ)。
    #[error("クラッシュレポートが大きすぎるため送信できませんでした")]
    PayloadTooLarge,
    /// 429 `rate_limited`(`submit_crash_report`のみ)。
    #[error("短時間に送信しすぎています。しばらく時間をおいてから送り直してください")]
    RateLimited,
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
        (409, "fallback_unavailable") => TrainApiError::FallbackUnavailable,
        (503, "minecraft_unavailable") => TrainApiError::MinecraftUnavailable,
        (403, "minecraft_identity_mismatch") => TrainApiError::MinecraftIdentityMismatch,
        (403, "link_required" | "login_denied") => TrainApiError::FallbackDenied,
        (401, _) => TrainApiError::Unauthorized,
        (403, "not_linked") => TrainApiError::NotLinked,
        (403, _) => TrainApiError::Forbidden,
        (404, "not_configured") => TrainApiError::NotConfigured,
        (404, _) => TrainApiError::NotFound,
        (405, _) => TrainApiError::MethodNotAllowed,
        (409, "already_linked") => TrainApiError::AlreadyLinked,
        (409, "uuid_already_linked") => TrainApiError::UuidAlreadyLinked,
        (413, _) => TrainApiError::PayloadTooLarge,
        (422, "not_connected") => TrainApiError::NotConnected,
        (429, _) => TrainApiError::RateLimited,
        (400, _) => TrainApiError::InvalidRequest,
        _ => TrainApiError::Api {
            status,
            code,
        },
    }
}

async fn map_credential_error(response: reqwest::Response) -> TrainApiError {
    match map_error_response(response).await {
        // A nonconforming server must not echo proof data back through an IPC error.
        TrainApiError::Api { status, .. } => TrainApiError::Api {
            status,
            code: "unexpected_response".into(),
        },
        error => error,
    }
}

/// TRAiNバックエンドAPIクライアントの振る舞いを定義するトレイト。
#[async_trait]
pub trait TrainApiClient: Send + Sync {
    async fn fallback_status(&self, _server_id: &str) -> Result<fallback::FallbackStatus, TrainApiError> {
        Ok(fallback::FallbackStatus {
            available: false, enabled_until: 0, protocol: fallback::PROTOCOL.into(),
        })
    }

    async fn enroll_fallback(&self, _server_id: &str, _request: &fallback::CredentialRequest)
        -> Result<fallback::CredentialReceipt, TrainApiError> {
        Err(TrainApiError::FallbackUnavailable)
    }

    async fn fallback_ticket(&self, _server_id: &str, _credential_id: &str)
        -> Result<fallback::FallbackTicket, TrainApiError> {
        Err(TrainApiError::FallbackUnavailable)
    }

    /// Discordサインイン後、ユーザーが所属しているTRAiN管理サーバー一覧を取得する。
    async fn get_member_servers(
        &self,
        discord_user_id: &str,
    ) -> Result<Vec<MemberServer>, TrainApiError>;

    /// 指定サーバーの設定(接続先・Mod構成・リソースパックなど)を取得する。
    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError>;

    /// MinecraftアクセストークンをRust内で取得・更新し、Discordアカウントとの紐づけを
    /// 申請する。サーバーはMinecraft APIで毎回所有権とUUID・名前を検証する。
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

    /// そのサーバーへ接続中に起きたクライアント側のクラッシュを報告する。
    ///
    /// 呼び出し元は事前に、送る内容をユーザーへ提示して同意を得ること
    /// (同意の無い自動送信はしない)。接続中だったかはTRAiN側が参加履歴で確かめる。
    async fn submit_crash_report(
        &self,
        server_id: &str,
        report: &CrashReportSubmission,
    ) -> Result<CrashReportReceipt, TrainApiError>;
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
            environment: None,
            parent_server_id: None,
            expires_at: None,
            style: None,
        }])
    }

    async fn get_server_config(&self, server_id: &str) -> Result<ServerConfig, TrainApiError> {
        Ok(ServerConfig {
            server_id: server_id.to_string(),
            address: "play.example.com:25565".to_string(),
            minecraft_version: "1.21".to_string(),
            mod_loader: Some("fabric".to_string()),
            fabric_loader_version: None,
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
        request: &LinkAccountRequest,
    ) -> Result<(), TrainApiError> {
        request.body()?;
        // モックは実際の所有権を確認しないが、証明が欠けたリクエストは成功させない。
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

    async fn submit_crash_report(
        &self,
        _server_id: &str,
        _report: &CrashReportSubmission,
    ) -> Result<CrashReportReceipt, TrainApiError> {
        // モック環境では常に成功させる(同意〜送信完了までの流れをUI上で確認できるようにする)。
        Ok(CrashReportReceipt {
            id: "mock-crash-report-1".to_string(),
        })
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

    fn credential_post(&self, path: &str) -> Result<reqwest::RequestBuilder, TrainApiError> {
        let url = reqwest::Url::parse(&format!("{}{path}", self.base_url))
            .map_err(|_| TrainApiError::InsecureCredentialTransport)?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost" || host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
        });
        if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
            || !url.username().is_empty() || url.password().is_some()
            || url.query().is_some() || url.fragment().is_some()
        {
            return Err(TrainApiError::InsecureCredentialTransport);
        }
        let builder = reqwest::Client::builder()
            .https_only(url.scheme() == "https")
            .redirect(reqwest::redirect::Policy::none());
        let client = if loopback { builder.no_proxy() } else { builder }.build()?;
        let request = client.post(url);
        Ok(match &self.discord_access_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        })
    }
}

#[async_trait]
impl TrainApiClient for HttpTrainApiClient {
    async fn fallback_status(&self, server_id: &str) -> Result<fallback::FallbackStatus, TrainApiError> {
        let response = self.get(&format!("/api/servers/{server_id}/auth-fallback/status")).send().await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json().await?)
    }

    async fn enroll_fallback(&self, server_id: &str, request: &fallback::CredentialRequest)
        -> Result<fallback::CredentialReceipt, TrainApiError> {
        let response = self.credential_post(&format!("/api/servers/{server_id}/auth-fallback/credentials"))?
            .json(request).send().await?;
        if !response.status().is_success() {
            return Err(map_credential_error(response).await);
        }
        Ok(response.json().await?)
    }

    async fn fallback_ticket(&self, server_id: &str, credential_id: &str)
        -> Result<fallback::FallbackTicket, TrainApiError> {
        let response = self.post(&format!("/api/servers/{server_id}/auth-fallback/tickets"))
            .json(&serde_json::json!({ "credential_id": credential_id })).send().await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json().await?)
    }

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
        let body = request.body()?;
        let response = self
            .credential_post(&format!("/api/servers/{server_id}/link"))?
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(map_credential_error(response).await);
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

    async fn submit_crash_report(
        &self,
        server_id: &str,
        report: &CrashReportSubmission,
    ) -> Result<CrashReportReceipt, TrainApiError> {
        let response = self
            .post(&format!("/api/servers/{server_id}/crash-reports"))
            .json(report)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(map_error_response(response).await);
        }
        Ok(response.json::<CrashReportReceipt>().await?)
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
    use super::*;
    use super::fallback::tests::server;

    fn link_request() -> LinkAccountRequest {
        LinkAccountRequest {
            mc_uuid: "00000000-0000-4000-8000-000000000001".into(),
            mc_name: "Player".into(),
            minecraft_access_token: "synthetic-minecraft-proof".into(),
        }
    }

    #[tokio::test]
    async fn normal_link_sends_discord_and_exact_ownership_proof_body() {
        let (client, captured) = server(200, r#"{"linked":true}"#).await;
        let request = link_request();
        client.link_account("server", &request).await.unwrap();
        let raw = captured.await.unwrap();
        assert!(raw.starts_with("POST /api/servers/server/link "));
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(headers.to_lowercase().contains("authorization: bearer discord-proof"));
        assert!(!headers.contains(&request.minecraft_access_token));
        assert!(body.len() <= 4096);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(body).unwrap(),
            serde_json::json!({
                "mc_uuid": request.mc_uuid, "mc_name": request.mc_name,
                "minecraft_access_token": request.minecraft_access_token,
            })
        );
        assert!(!format!("{request:?}").contains(&request.minecraft_access_token));
    }

    #[tokio::test]
    async fn link_proof_errors_remain_distinct_from_discord_and_uniqueness_errors() {
        for (status, body, expected) in [
            (400, r#"{"error":"invalid_body"}"#, "invalid"),
            (403, r#"{"error":"minecraft_identity_mismatch"}"#, "minecraft"),
            (503, r#"{"error":"minecraft_unavailable"}"#, "unavailable"),
            (401, r#"{"error":"unauthorized"}"#, "discord"),
            (404, r#"{"error":"not_found"}"#, "hidden"),
            (409, r#"{"error":"already_linked"}"#, "discord-conflict"),
            (409, r#"{"error":"uuid_already_linked"}"#, "minecraft-conflict"),
        ] {
            let (client, captured) = server(status, body).await;
            let error = client.link_account("server", &link_request()).await.unwrap_err();
            assert!(matches!((expected, error),
                ("invalid", TrainApiError::InvalidRequest)
                | ("minecraft", TrainApiError::MinecraftIdentityMismatch)
                | ("unavailable", TrainApiError::MinecraftUnavailable)
                | ("discord", TrainApiError::Unauthorized)
                | ("hidden", TrainApiError::NotFound)
                | ("discord-conflict", TrainApiError::AlreadyLinked)
                | ("minecraft-conflict", TrainApiError::UuidAlreadyLinked)
            ));
            assert!(captured.await.unwrap().starts_with("POST /api/servers/server/link "));
        }
        let (client, captured) = server(418, r#"{"error":"synthetic-minecraft-proof"}"#).await;
        let error = client.link_account("server", &link_request()).await.unwrap_err();
        assert!(!error.to_string().contains("synthetic-minecraft-proof"));
        assert!(!format!("{error:?}").contains("synthetic-minecraft-proof"));
        captured.await.unwrap();
    }

    #[tokio::test]
    async fn missing_or_oversized_proof_fails_before_http_and_in_mock() {
        let client = HttpTrainApiClient::new("http://127.0.0.1:1", None);
        for proof in ["".into(), " \t\n".into(), "x".repeat(3073), "\"".repeat(3072)] {
            let mut request = link_request();
            request.minecraft_access_token = proof;
            for client in [&client as &dyn TrainApiClient, &MockTrainApiClient] {
                assert!(matches!(client.link_account("server", &request).await, Err(TrainApiError::InvalidRequest)));
            }
        }
        let mut request = link_request();
        request.minecraft_access_token = "x".into();
        MockTrainApiClient.link_account("server", &request).await.unwrap();
        request.minecraft_access_token = "x".repeat(3072);
        assert!(request.body().unwrap().len() <= 4096);
        MockTrainApiClient.link_account("server", &request).await.unwrap();
        let escapes = 4096 - request.body().unwrap().len();
        request.minecraft_access_token.replace_range(..escapes, &"\"".repeat(escapes));
        assert_eq!(request.body().unwrap().len(), 4096);
        request.minecraft_access_token.replace_range(escapes..escapes + 1, "\"");
        assert!(matches!(request.body(), Err(TrainApiError::InvalidRequest)));
    }

    #[test]
    fn ownership_proof_transport_rejects_plaintext_remote_and_url_credentials() {
        for base in [
            "http://example.com", "http://192.0.2.1", "http://localhost.example.com",
            "ftp://example.com", "https://user:password@example.com",
            "https://example.com?token=untrusted", "https://example.com#fragment", "not a url",
        ] {
            let client = HttpTrainApiClient::new(base, None);
            assert!(matches!(client.credential_post("/api/servers/server/link"), Err(TrainApiError::InsecureCredentialTransport)));
        }
        for base in ["https://example.com", "http://localhost:8787", "http://127.0.0.1:8787", "http://[::1]:8787"] {
            let client = HttpTrainApiClient::new(base, None);
            assert!(client.credential_post("/api/servers/server/link").is_ok());
        }
    }

    #[tokio::test]
    async fn proof_is_never_redirected_even_to_another_loopback_endpoint() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;
        use tokio::time::{timeout, Duration};
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_url = format!("http://{}/capture", target.local_addr().unwrap());
        let client = HttpTrainApiClient::new(format!("http://{}", origin.local_addr().unwrap()), Some("discord-proof".into()));
        let redirect = tokio::spawn(async move {
            let (mut stream, _) = origin.accept().await.unwrap();
            let mut bytes = [0; 8192];
            assert!(stream.read(&mut bytes).await.unwrap() > 0);
            stream.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        });
        let capture = tokio::spawn(async move {
            if let Ok(Ok((mut stream, _))) = timeout(Duration::from_millis(500), target.accept()).await {
                let mut bytes = [0; 8192];
                stream.read(&mut bytes).await.unwrap();
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
                return true;
            }
            false
        });
        let error = client.link_account("server", &link_request()).await.unwrap_err();
        assert!(matches!(error, TrainApiError::Api { status: 307, .. }));
        redirect.await.unwrap();
        assert!(!capture.await.unwrap());
    }

    #[tokio::test]
    async fn member_server_http_response_preserves_workspace_metadata_for_ipc() {
        use super::{HttpTrainApiClient, TrainApiClient};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let body = include_str!("../tests/fixtures/member-servers.json");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = HttpTrainApiClient::new(
            format!("http://{}", listener.local_addr().unwrap()),
            Some("discord-proof".into()),
        );
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.windows(4).any(|v| v == b"\r\n\r\n") {
                let mut buffer = [0; 1024];
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /api/members/member/servers "));
            assert!(request.to_lowercase().contains("authorization: bearer discord-proof"));
            stream.write_all(format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ).as_bytes()).await.unwrap();
        });
        let servers = client.get_member_servers("member").await.unwrap();
        task.await.unwrap();
        assert_eq!(
            serde_json::to_value(servers).unwrap(),
            serde_json::from_str::<serde_json::Value>(body).unwrap()
        );
    }

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

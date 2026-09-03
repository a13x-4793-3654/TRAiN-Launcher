//! Mojangが公開するバージョンマニフェスト (version_manifest_v2.json) の取得。

use serde::{Deserialize, Serialize};

use crate::CoreError;

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// バージョンマニフェスト内の1バージョンエントリ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
}

/// 最新リリース/スナップショットのバージョンID。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

/// バージョンマニフェスト全体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

/// Mojangのバージョンマニフェストを取得する。
///
/// TODO: reqwestでのHTTP取得・ローカルキャッシュ(ETag等)を実装する。
pub async fn fetch_version_manifest() -> Result<VersionManifest, CoreError> {
    let _ = VERSION_MANIFEST_URL;
    Err(CoreError::NotImplemented(
        "version_manifest::fetch_version_manifest",
    ))
}

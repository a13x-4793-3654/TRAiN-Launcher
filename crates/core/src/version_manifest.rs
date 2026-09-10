//! Mojangが公開するバージョンマニフェスト (version_manifest_v2.json) と、
//! 個々のバージョンの詳細情報(ライブラリ・ダウンロードURL・起動引数等)の取得。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::rules::Rule;
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
    #[serde(default)]
    pub sha1: Option<String>,
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
pub async fn fetch_version_manifest() -> Result<VersionManifest, CoreError> {
    let client = reqwest::Client::new();
    let manifest = client
        .get(VERSION_MANIFEST_URL)
        .send()
        .await?
        .error_for_status()?
        .json::<VersionManifest>()
        .await?;
    Ok(manifest)
}

/// マニフェストから指定バージョンIDのエントリを探す。
pub fn find_version<'a>(
    manifest: &'a VersionManifest,
    version_id: &str,
) -> Option<&'a VersionEntry> {
    manifest.versions.iter().find(|entry| entry.id == version_id)
}

/// バージョン固有の詳細情報 (`https://piston-meta.mojang.com/.../<id>.json` の内容)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDetails {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    #[serde(rename = "assetIndex")]
    pub asset_index: AssetIndexRef,
    pub assets: String,
    pub downloads: VersionDownloads,
    #[serde(default)]
    pub libraries: Vec<Library>,
    /// 1.13以降: 構造化された条件付き引数。
    #[serde(default)]
    pub arguments: Option<Arguments>,
    /// 1.12以前: スペース区切りの単純な引数文字列(プレースホルダーのみ、条件分岐は無い)。
    #[serde(default, rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    #[serde(default, rename = "totalSize")]
    pub total_size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDownloads {
    pub client: Artifact,
    #[serde(default)]
    pub server: Option<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaVersion {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

/// ダウンロード可能な1ファイル(URL・SHA1・サイズ)。ライブラリの場合は加えて
/// `libraries/` からの相対パス(`path`)を持つ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(default)]
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    /// レガシー(LWJGL2世代)ネイティブライブラリ: OS名 → `downloads.classifiers` のキー。
    /// `${arch}` は32/64ビット表記に置換される。
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<ExtractRules>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractRules {
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// JVM/ゲーム起動引数(1.13以降の構造化形式)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<ArgumentEntry>,
    #[serde(default)]
    pub jvm: Vec<ArgumentEntry>,
}

/// 引数1要素。単純な文字列、または `rules` による条件付き値のいずれか。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentEntry {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ArgumentValue,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgumentValue {
    Single(String),
    Multiple(Vec<String>),
}

/// アセットインデックス (`assets/indexes/<id>.json`) の内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// バージョンマニフェストのエントリからバージョン詳細JSONを取得する。
pub async fn fetch_version_details(entry: &VersionEntry) -> Result<VersionDetails, CoreError> {
    let client = reqwest::Client::new();
    let details = client
        .get(&entry.url)
        .send()
        .await?
        .error_for_status()?
        .json::<VersionDetails>()
        .await?;
    Ok(details)
}

/// アセットインデックスJSONを取得する。
pub async fn fetch_asset_index(asset_index: &AssetIndexRef) -> Result<AssetIndex, CoreError> {
    let client = reqwest::Client::new();
    let index = client
        .get(&asset_index.url)
        .send()
        .await?
        .error_for_status()?
        .json::<AssetIndex>()
        .await?;
    Ok(index)
}

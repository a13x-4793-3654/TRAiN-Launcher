//! Mod依存関係の自動解決、および解決済みファイルのダウンロード。
//!
//! `libium`/`ferium` (<https://github.com/gorilla-devs/ferium>) の依存解決ロジックを参考に、
//! Modrinth/CurseForgeの依存関係APIを再帰的に辿ってインストール対象を確定させる。

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use crate::{curseforge, modrinth, ModsError};

/// Mod/リソースパックの提供元。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModProvider {
    Modrinth,
    CurseForge,
    /// TRAiNサーバー設定など、呼び出し元が既にバージョン適合/再配布可否を確認済みの
    /// 直接ダウンロードURL(Modrinth/CurseForgeのプロジェクトIDを持たない)。
    Direct,
}

/// URLにバージョンが明示されていない場合に、互換性のある最新バージョンを選ぶための絞り込み条件。
#[derive(Debug, Clone, Default)]
pub struct ResolveFilter {
    /// 対象のMinecraftバージョン(例: `"1.20.4"`)。`None` の場合は絞り込まない。
    pub minecraft_version: Option<String>,
    /// 対象のModローダー(例: `"fabric"`、`"forge"`)。`None` の場合は絞り込まない。
    pub mod_loader: Option<String>,
}

/// 解決済みの1ファイル(Mod本体またはリソースパック)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFile {
    pub provider: ModProvider,
    /// Modrinthの場合はプロジェクトID、CurseForgeの場合は数値のMod IDを文字列化したもの。
    pub project_id: String,
    pub project_name: String,
    /// Modrinthの場合はバージョンID、CurseForgeの場合は数値のファイルIDを文字列化したもの。
    pub version_id: String,
    pub filename: String,
    pub download_url: String,
    /// CurseForgeはファイルによってはSHA1を提供しないため任意項目。
    pub sha1: Option<String>,
}

/// 既に解決済みの直接ダウンロードURLを、`download_resolved_file`にそのまま渡せる
/// `ResolvedFile`へ変換する。
///
/// TRAiNサーバー設定の`mod_urls`/`resource_pack_urls`は、Modrinth/CurseForgeの
/// プロジェクトページURLではなく、サーバー側で既にバージョン適合・再配布可否の確認を
/// 終えた直接ダウンロードURL。`resolve_dependencies`/`resource_pack::resolve_from_url`
/// (Modrinth/CurseForgeのプロジェクトID・スラッグとして解釈する)へ渡すと誤認識されて
/// 404になるため、解決を経由せずここでダウンロード対象として扱う。
pub fn resolved_file_from_direct_url(url: &str) -> ResolvedFile {
    let raw_segment = url
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or("download");
    // クエリ文字列(署名付きURL等)を除いたパス部分のみをファイル名として扱う。
    let raw_segment = raw_segment.split(['?', '#']).next().unwrap_or(raw_segment);
    // URLパスは `%2B`(`+`)・`%20`(半角スペース)等をパーセントエンコードして含むため、
    // デコードせずファイル名に使うと同一ファイルなのに毎回異なる名前で保存されてしまう。
    // これによりMod本体は同一でもファイル名違いの重複ファイルが並存し、Fabricが
    // 「重複したMod ID」としてゲーム起動を拒否する不具合につながっていた(要修正点)。
    let filename = percent_decode(raw_segment);
    ResolvedFile {
        provider: ModProvider::Direct,
        project_id: url.to_string(),
        project_name: filename.clone(),
        version_id: String::new(),
        filename,
        download_url: url.to_string(),
        sha1: None,
    }
}

/// URLパスセグメントの `%XX` パーセントエンコーディングをデコードする(ファイル名専用の
/// ため、クエリ文字列と異なり `+` はスペースへ変換しない)。不正なUTF-8になった場合は
/// 元の文字列をそのまま返す。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                decoded.push(((hi << 4) | lo) as u8);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| input.to_string())
}

/// 依存関係解決のための、他プロジェクトへの参照(URLではなく提供元+IDで表現する)。
#[derive(Debug, Clone)]
pub enum DependencyRef {
    Modrinth(String),
    CurseForge(i32),
}

/// 解決対象のMod参照(ユーザーが入力したURL、またはプロジェクトID/スラッグ)。
#[derive(Debug, Clone)]
pub struct ModReference {
    pub url: String,
}

/// 解決処理中の内部キュー項目。
enum QueueItem {
    Reference(String),
    Modrinth(String),
    CurseForge(i32),
}

fn dedup_key(file: &ResolvedFile) -> (ModProvider, String) {
    (file.provider, file.project_id.clone())
}

/// URLかどうかを問わず、1件の参照を解決し、解決済みファイルとその必須依存関係を返す。
///
/// 提供元の判定は次の優先順位で行う:
/// 1. `curseforge.com` を含むURL → CurseForge
/// 2. `modrinth.com` を含むURL → Modrinth
/// 3. URLでなく数値としてパースできる → CurseForgeの数値Mod ID
/// 4. それ以外 → ModrinthのプロジェクトID/スラッグ
async fn resolve_one(
    reference: &str,
    filter: &ResolveFilter,
    curseforge_api_key: Option<&str>,
) -> Result<(ResolvedFile, Vec<DependencyRef>), ModsError> {
    if reference.contains("curseforge.com") {
        let api_key = curseforge_api_key.ok_or(ModsError::ApiKeyMissing)?;
        let file = curseforge::resolve_from_url(reference, api_key, filter).await?;
        let deps = curseforge::required_dependencies(api_key, &file).await?;
        return Ok((file, deps));
    }
    if reference.contains("modrinth.com") {
        let file = modrinth::resolve_from_url(reference, filter).await?;
        let deps = modrinth::required_dependencies(&file.version_id).await?;
        return Ok((file, deps));
    }
    if let Ok(mod_id) = reference.parse::<i32>() {
        let api_key = curseforge_api_key.ok_or(ModsError::ApiKeyMissing)?;
        let file = curseforge::resolve_from_mod_id(mod_id, api_key, filter).await?;
        let deps = curseforge::required_dependencies(api_key, &file).await?;
        return Ok((file, deps));
    }
    let file = modrinth::resolve_from_url(reference, filter).await?;
    let deps = modrinth::required_dependencies(&file.version_id).await?;
    Ok((file, deps))
}

/// 依存関係(`Required`のみ)を再帰的に解決し、リクエストされたMod自身を含めた
/// インストール対象一覧を返す(重複するプロジェクトは1つにまとめる)。
///
/// 戻り値の先頭要素は `requested` の1件目に対応する(リクエスト順・幅優先で解決するため)。
/// CurseForgeの依存関係を解決するには `curseforge_api_key` が必要
/// (Modrinthのみで完結する場合は `None` でよい)。
pub async fn resolve_dependencies(
    requested: Vec<ModReference>,
    filter: &ResolveFilter,
    curseforge_api_key: Option<&str>,
) -> Result<Vec<ResolvedFile>, ModsError> {
    let mut resolved = Vec::new();
    let mut seen: HashSet<(ModProvider, String)> = HashSet::new();
    let mut queue: VecDeque<QueueItem> = requested
        .into_iter()
        .map(|reference| QueueItem::Reference(reference.url))
        .collect();

    while let Some(item) = queue.pop_front() {
        let reference = match &item {
            QueueItem::Reference(url) => url.clone(),
            QueueItem::Modrinth(project_id) => project_id.clone(),
            QueueItem::CurseForge(mod_id) => mod_id.to_string(),
        };
        let (file, deps) = resolve_one(&reference, filter, curseforge_api_key).await?;

        if !seen.insert(dedup_key(&file)) {
            continue;
        }
        for dep in deps {
            match dep {
                DependencyRef::Modrinth(project_id) => {
                    queue.push_back(QueueItem::Modrinth(project_id));
                }
                DependencyRef::CurseForge(mod_id) => {
                    queue.push_back(QueueItem::CurseForge(mod_id));
                }
            }
        }
        resolved.push(file);
    }

    Ok(resolved)
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 解決済みファイルをダウンロードし、`dest_dir` に保存する(ディレクトリが無ければ作成する)。
///
/// SHA1が判明していれば整合性を検証する(CurseForgeはSHA1を提供しないファイルもあるため、
/// その場合は検証をスキップする)。既に同一内容のファイルが存在する場合はダウンロードを
/// スキップする。
pub async fn download_resolved_file(
    resolved: &ResolvedFile,
    dest_dir: &Path,
) -> Result<PathBuf, ModsError> {
    tokio::fs::create_dir_all(dest_dir).await?;
    let dest = dest_dir.join(&resolved.filename);

    if let Some(expected_sha1) = &resolved.sha1 {
        if let Ok(existing) = tokio::fs::read(&dest).await {
            if &sha1_hex(&existing) == expected_sha1 {
                return Ok(dest);
            }
        }
    }

    let bytes = reqwest::get(&resolved.download_url)
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    if let Some(expected_sha1) = &resolved.sha1 {
        let actual = sha1_hex(&bytes);
        if &actual != expected_sha1 {
            return Err(ModsError::HashMismatch {
                url: resolved.download_url.clone(),
                expected: expected_sha1.clone(),
                actual,
            });
        }
    }

    tokio::fs::write(&dest, &bytes).await?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_file_from_direct_url_decodes_percent_encoded_filename() {
        let resolved = resolved_file_from_direct_url(
            "https://cdn.modrinth.com/data/xxxx/versions/yyyy/bettercombat-fabric-2.4.0%2B1.21.1.jar",
        );
        assert_eq!(resolved.filename, "bettercombat-fabric-2.4.0+1.21.1.jar");
        assert_eq!(resolved.provider, ModProvider::Direct);
    }

    #[test]
    fn resolved_file_from_direct_url_decodes_spaces() {
        let resolved =
            resolved_file_from_direct_url("https://cdn.example.com/mods/Icons%20v.1.13.4.zip");
        assert_eq!(resolved.filename, "Icons v.1.13.4.zip");
    }

    #[test]
    fn resolved_file_from_direct_url_strips_query_string() {
        let resolved = resolved_file_from_direct_url(
            "https://cdn.example.com/mods/example.jar?X-Amz-Signature=abc123",
        );
        assert_eq!(resolved.filename, "example.jar");
    }

    #[test]
    fn resolved_file_from_direct_url_leaves_plain_filename_unchanged() {
        let resolved = resolved_file_from_direct_url("https://cdn.example.com/mods/plain.jar");
        assert_eq!(resolved.filename, "plain.jar");
    }
}

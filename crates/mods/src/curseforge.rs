//! CurseForge API クライアント (`furse` crateを使用)。
//!
//! CurseForgeの公開Web URL (`https://www.curseforge.com/minecraft/mc-mods/<slug>` 等)は
//! スラッグ形式だが、公式APIはMod ID(数値)を要求する。`furse` はスラッグ検索エンドポイントを
//! ラップしていないため、ここでは直接HTTPリクエストを組み立てて解決する。

use furse::structures::file_structs::{File, FileRelationType, HashAlgo};
use furse::structures::mod_structs::Mod;
use furse::Furse;
use serde::Deserialize;

use crate::resolver::{sanitize_filename, DependencyRef, ModProvider, ResolveFilter, ResolvedFile};
use crate::ModsError;

/// CurseForge上でのMinecraft(Java版)のゲームID。
const MINECRAFT_GAME_ID: i32 = 432;

/// CurseForge APIクライアントを構築する。
///
/// APIキーは <https://console.curseforge.com/#/api-keys> で発行し、環境変数
/// `TRAIN_LAUNCHER_CURSEFORGE_API_KEY` から読み込む(`crate::config::curseforge_api_key_from_env`)。
pub fn build_client(api_key: &str) -> Furse {
    Furse::new(api_key.to_string())
}

/// URLから抽出したCurseForgeプロジェクト参照。
struct ParsedRef {
    slug: String,
    class_id: Option<i32>,
    file_id: Option<i32>,
}

/// URLパスの2番目のセグメント(カテゴリ)からCurseForgeのclassIdへの対応表。
fn class_id_for_segment(segment: &str) -> Option<i32> {
    match segment {
        "mc-mods" => Some(6),
        "texture-packs" => Some(12),
        "modpacks" => Some(4471),
        "worlds" => Some(17),
        "bukkit-plugins" => Some(5),
        "customization" => Some(4546),
        "shaders" => Some(6552),
        "data-packs" => Some(6945),
        _ => None,
    }
}

/// `https://www.curseforge.com/minecraft/<カテゴリ>/<スラッグ>[/files/<ファイルID>]` 形式の
/// URLを解析する。
fn parse_reference(input: &str) -> Result<ParsedRef, ModsError> {
    let trimmed = input.trim();
    let url = url::Url::parse(trimmed).map_err(|_| ModsError::InvalidUrl(trimmed.to_string()))?;
    let segments: Vec<&str> = url
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default();
    if segments.len() < 3 {
        return Err(ModsError::InvalidUrl(trimmed.to_string()));
    }
    let class_id = class_id_for_segment(segments[1]);
    let slug = segments[2].to_string();
    let file_id = if segments.len() >= 5 && segments[3] == "files" {
        segments[4].parse::<i32>().ok()
    } else {
        None
    };
    Ok(ParsedRef {
        slug,
        class_id,
        file_id,
    })
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Vec<Mod>,
}

/// `furse` が提供しないスラッグ検索エンドポイントを直接呼び出し、CurseForgeのMod情報を取得する。
async fn find_mod_by_slug(
    api_key: &str,
    slug: &str,
    class_id: Option<i32>,
) -> Result<Mod, ModsError> {
    let mut url = url::Url::parse("https://api.curseforge.com/v1/mods/search")
        .expect("static URL must be valid");
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("gameId", &MINECRAFT_GAME_ID.to_string());
        query.append_pair("slug", slug);
        if let Some(class_id) = class_id {
            query.append_pair("classId", &class_id.to_string());
        }
    }
    let response: SearchResponse = reqwest::Client::new()
        .get(url)
        .header("x-api-key", api_key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    response
        .data
        .into_iter()
        .next()
        .ok_or_else(|| ModsError::NotFound(format!("CurseForge mod not found for slug: {slug}")))
}

/// ファイル一覧の中から `filter` に合致するファイルを選ぶ(ファイルIDが大きいほど新しいとみなす)。
fn pick_file(files: &[File], filter: &ResolveFilter) -> Option<File> {
    files
        .iter()
        .filter(|file| match &filter.minecraft_version {
            Some(version) => file.game_versions.iter().any(|v| v == version),
            None => true,
        })
        .max_by_key(|file| file.id)
        .cloned()
}

fn to_resolved_file(mod_info: &Mod, file: &File) -> Result<ResolvedFile, ModsError> {
    let download_url = file.download_url.clone().ok_or_else(|| {
        ModsError::NotFound(format!(
            "file {} has no direct download URL (author disabled third-party downloads)",
            file.id
        ))
    })?;
    let sha1 = file
        .hashes
        .iter()
        .find(|hash| matches!(hash.algo, HashAlgo::Sha1))
        .map(|hash| hash.value.clone());

    Ok(ResolvedFile {
        provider: ModProvider::CurseForge,
        project_id: mod_info.id.to_string(),
        project_name: mod_info.name.clone(),
        version_id: file.id.to_string(),
        // `file_name`はCurseForge API(第三者がアップロードしたファイル名を含み得る)由来
        // のため、パス区切り文字・`..`を含んでいないか正規化してから使用する
        // (パストラバーサル対策、詳細は`sanitize_filename`のドキュメント参照)。
        filename: sanitize_filename(&file.file_name),
        download_url: download_url.to_string(),
        sha1,
    })
}

async fn resolve_for_mod(
    mod_info: &Mod,
    file_id: Option<i32>,
    api_key: &str,
    filter: &ResolveFilter,
) -> Result<ResolvedFile, ModsError> {
    let client = build_client(api_key);
    let file = if let Some(file_id) = file_id {
        client.get_mod_file(mod_info.id, file_id).await?
    } else {
        pick_file(&mod_info.latest_files, filter).ok_or_else(|| {
            ModsError::NotFound(format!(
                "no compatible file found for CurseForge mod: {}",
                mod_info.name
            ))
        })?
    };
    to_resolved_file(mod_info, &file)
}

/// URLからModのファイル情報を解決する。
pub async fn resolve_from_url(
    url: &str,
    api_key: &str,
    filter: &ResolveFilter,
) -> Result<ResolvedFile, ModsError> {
    let parsed = parse_reference(url)?;
    let mod_info = find_mod_by_slug(api_key, &parsed.slug, parsed.class_id).await?;
    resolve_for_mod(&mod_info, parsed.file_id, api_key, filter).await
}

/// 数値のMod IDから直接解決する(依存関係解決時、既にIDが判明している場合に使用)。
pub async fn resolve_from_mod_id(
    mod_id: i32,
    api_key: &str,
    filter: &ResolveFilter,
) -> Result<ResolvedFile, ModsError> {
    let client = build_client(api_key);
    let mod_info = client.get_mod(mod_id).await?;
    resolve_for_mod(&mod_info, None, api_key, filter).await
}

/// 解決済みファイルの必須依存関係(`RequiredDependency`)のMod ID一覧を返す。
pub async fn required_dependencies(
    api_key: &str,
    resolved: &ResolvedFile,
) -> Result<Vec<DependencyRef>, ModsError> {
    let mod_id: i32 = resolved.project_id.parse().map_err(|_| {
        ModsError::NotFound(format!("invalid CurseForge mod id: {}", resolved.project_id))
    })?;
    let file_id: i32 = resolved.version_id.parse().map_err(|_| {
        ModsError::NotFound(format!(
            "invalid CurseForge file id: {}",
            resolved.version_id
        ))
    })?;
    let client = build_client(api_key);
    let file = client.get_mod_file(mod_id, file_id).await?;
    Ok(file
        .dependencies
        .iter()
        .filter(|dependency| {
            matches!(dependency.relation_type, FileRelationType::RequiredDependency)
        })
        .map(|dependency| DependencyRef::CurseForge(dependency.mod_id))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reference_extracts_slug_and_class_id() {
        let parsed =
            parse_reference("https://www.curseforge.com/minecraft/mc-mods/jei").unwrap();
        assert_eq!(parsed.slug, "jei");
        assert_eq!(parsed.class_id, Some(6));
        assert_eq!(parsed.file_id, None);
    }

    #[test]
    fn parse_reference_extracts_file_id_from_files_url() {
        let parsed = parse_reference(
            "https://www.curseforge.com/minecraft/mc-mods/jei/files/1234567",
        )
        .unwrap();
        assert_eq!(parsed.slug, "jei");
        assert_eq!(parsed.file_id, Some(1234567));
    }

    #[test]
    fn parse_reference_rejects_non_url_input() {
        assert!(parse_reference("jei").is_err());
    }

    #[test]
    fn class_id_for_segment_maps_known_categories() {
        assert_eq!(class_id_for_segment("mc-mods"), Some(6));
        assert_eq!(class_id_for_segment("texture-packs"), Some(12));
        assert_eq!(class_id_for_segment("unknown-category"), None);
    }
}

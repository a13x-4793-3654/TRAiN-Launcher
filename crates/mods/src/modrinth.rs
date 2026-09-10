//! Modrinth API クライアント (`ferinth` crateを使用)。

use ferinth::structures::version::{DependencyType, Version, VersionFile};
use ferinth::Ferinth;

use crate::resolver::{DependencyRef, ModProvider, ResolveFilter, ResolvedFile};
use crate::ModsError;

/// Modrinth APIクライアント(未認証)を構築する。公開データの取得のみであれば認証不要。
pub fn build_client() -> Ferinth<()> {
    Ferinth::<()>::new(
        env!("CARGO_PKG_NAME"),
        Some(env!("CARGO_PKG_VERSION")),
        None,
    )
}

/// URLから抽出したModrinthプロジェクト参照。
struct ParsedRef {
    /// プロジェクトID、またはスラッグ(`Ferinth`はどちらも受け付ける)。
    project: String,
    /// URLにバージョンが含まれていた場合のバージョン番号(`version_get_from_number` で使用)。
    version_number: Option<String>,
}

/// `https://modrinth.com/<type>/<slug>[/version/<バージョン番号>]` 形式のURL、または
/// 素のプロジェクトID/スラッグを解析する。
fn parse_reference(input: &str) -> ParsedRef {
    let trimmed = input.trim();
    if let Ok(url) = url::Url::parse(trimmed) {
        let segments: Vec<&str> = url
            .path_segments()
            .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
            .unwrap_or_default();
        // 例: ["mod", "sodium"] または ["mod", "sodium", "version", "mc1.20-0.5.8"]
        if segments.len() >= 2 {
            let project = segments[1].to_string();
            let version_number = if segments.len() >= 4 && segments[2] == "version" {
                Some(segments[3].to_string())
            } else {
                None
            };
            return ParsedRef {
                project,
                version_number,
            };
        }
    }
    ParsedRef {
        project: trimmed.to_string(),
        version_number: None,
    }
}

/// バージョンの中からダウンロード対象のファイルを選ぶ(`primary` フラグ優先、無ければ先頭)。
fn pick_file(version: &Version) -> Result<&VersionFile, ModsError> {
    version
        .files
        .iter()
        .find(|file| file.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| {
            ModsError::NotFound(format!("no downloadable file for version {}", version.id))
        })
}

/// URL(またはプロジェクトID/スラッグ)からModのバージョン情報を解決する。
///
/// URLにバージョンが指定されている場合はそのバージョンを、指定されていない場合は
/// `filter` (Minecraftバージョン/Modローダー)に合致する最新バージョンを選択する。
pub async fn resolve_from_url(
    reference: &str,
    filter: &ResolveFilter,
) -> Result<ResolvedFile, ModsError> {
    let client = build_client();
    let parsed = parse_reference(reference);

    let version = if let Some(number) = &parsed.version_number {
        client
            .version_get_from_number(&parsed.project, number)
            .await?
    } else {
        let loaders = filter.mod_loader.as_deref().map(|loader| vec![loader]);
        let game_versions = filter
            .minecraft_version
            .as_deref()
            .map(|version| vec![version]);
        let versions = client
            .version_list_filtered(
                &parsed.project,
                loaders.as_deref(),
                game_versions.as_deref(),
                None,
            )
            .await?;
        versions.into_iter().next().ok_or_else(|| {
            ModsError::NotFound(format!(
                "no compatible Modrinth version found for project: {}",
                parsed.project
            ))
        })?
    };

    let project = client.project_get(&version.project_id).await?;
    let file = pick_file(&version)?;

    Ok(ResolvedFile {
        provider: ModProvider::Modrinth,
        project_id: version.project_id.clone(),
        project_name: project.title,
        version_id: version.id.clone(),
        filename: file.filename.clone(),
        download_url: file.url.to_string(),
        sha1: Some(file.hashes.sha1.clone()),
    })
}

/// 指定バージョンが必須とする依存関係(`Required`)のプロジェクトID一覧を返す。
pub async fn required_dependencies(version_id: &str) -> Result<Vec<DependencyRef>, ModsError> {
    let client = build_client();
    let version = client.version_get(version_id).await?;
    Ok(version
        .dependencies
        .iter()
        .filter(|dependency| dependency.dependency_type == DependencyType::Required)
        .filter_map(|dependency| dependency.project_id.clone())
        .map(DependencyRef::Modrinth)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reference_extracts_slug_from_project_url() {
        let parsed = parse_reference("https://modrinth.com/mod/sodium");
        assert_eq!(parsed.project, "sodium");
        assert_eq!(parsed.version_number, None);
    }

    #[test]
    fn parse_reference_extracts_version_number_from_version_url() {
        let parsed =
            parse_reference("https://modrinth.com/mod/sodium/version/mc1.20.4-0.5.8");
        assert_eq!(parsed.project, "sodium");
        assert_eq!(parsed.version_number, Some("mc1.20.4-0.5.8".to_string()));
    }

    #[test]
    fn parse_reference_falls_back_to_raw_project_id() {
        let parsed = parse_reference("AANobbMI");
        assert_eq!(parsed.project, "AANobbMI");
        assert_eq!(parsed.version_number, None);
    }
}

//! リソースパックの自動導入。
//!
//! Modrinth/CurseForge上ではリソースパックも「プロジェクト」として同じAPIで扱われるため、
//! Mod解決と全く同じ仕組み(`modrinth`/`curseforge`モジュール)で解決できる。
//! 導入先ディレクトリが `mods/` ではなく `resourcepacks/` になる点のみが異なる。

use std::path::{Path, PathBuf};

use crate::resolver::{self, ResolveFilter, ResolvedFile};
use crate::ModsError;

/// URL指定でリソースパックを解決する(バージョン絞り込みのみ、Modローダーの概念は
/// リソースパックには存在しないため指定不要)。
pub async fn resolve_from_url(
    url: &str,
    minecraft_version: Option<&str>,
    curseforge_api_key: Option<&str>,
) -> Result<ResolvedFile, ModsError> {
    let filter = ResolveFilter {
        minecraft_version: minecraft_version.map(str::to_string),
        mod_loader: None,
    };
    if url.contains("curseforge.com") {
        let api_key = curseforge_api_key.ok_or(ModsError::ApiKeyMissing)?;
        crate::curseforge::resolve_from_url(url, api_key, &filter).await
    } else {
        crate::modrinth::resolve_from_url(url, &filter).await
    }
}

/// URL指定でリソースパックを解決し、`dest_dir` (通常は `<.minecraft>/resourcepacks`) へ
/// ダウンロード・設置する。
pub async fn install_from_url(
    url: &str,
    minecraft_version: Option<&str>,
    curseforge_api_key: Option<&str>,
    dest_dir: &Path,
) -> Result<PathBuf, ModsError> {
    let resolved = resolve_from_url(url, minecraft_version, curseforge_api_key).await?;
    resolver::download_resolved_file(&resolved, dest_dir).await
}

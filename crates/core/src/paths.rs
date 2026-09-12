//! ランチャーが使用するディレクトリ構造の解決。
//!
//! 全プロファイル共通のゲームデータ(バージョンjar・ライブラリ・アセット)を1つの
//! ルートディレクトリ配下にまとめて管理する(一般的なMinecraftランチャーの `.minecraft`
//! ディレクトリ相当)。バージョンファイル自体はどのプロファイルからも共有されるため、
//! プロファイルごとに複製はしない。

use std::path::{Path, PathBuf};

/// ランチャーのルートディレクトリを起点とした各種パスを解決するヘルパー。
#[derive(Debug, Clone)]
pub struct LauncherPaths {
    root: PathBuf,
}

impl LauncherPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    pub fn version_dir(&self, version_id: &str) -> PathBuf {
        self.versions_dir().join(safe_path_component(version_id))
    }

    /// キャッシュ済みのバージョンJSON(取得したVersionDetailsの生データ)の保存先。
    /// 起動時に毎回Mojangへ問い合わせずに済むよう、ダウンロード時に保存しておく。
    pub fn version_json_path(&self, version_id: &str) -> PathBuf {
        let safe_id = safe_path_component(version_id);
        self.versions_dir().join(safe_id).join(format!("{safe_id}.json"))
    }

    pub fn version_jar_path(&self, version_id: &str) -> PathBuf {
        let safe_id = safe_path_component(version_id);
        self.versions_dir().join(safe_id).join(format!("{safe_id}.jar"))
    }

    /// レガシーバージョン(LWJGL2世代)向けに展開したネイティブライブラリの保存先。
    pub fn natives_dir(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id).join("natives")
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn library_path(&self, relative_path: &str) -> PathBuf {
        // Mojangのライブラリpathは常に `/` 区切りなので、OSのパス区切りに変換する。
        let mut path = self.libraries_dir();
        for segment in relative_path.split('/') {
            path.push(segment);
        }
        path
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }

    pub fn asset_index_path(&self, assets_id: &str) -> PathBuf {
        let safe_id = safe_path_component(assets_id);
        self.assets_dir().join("indexes").join(format!("{safe_id}.json"))
    }

    pub fn asset_object_path(&self, hash: &str) -> PathBuf {
        let prefix = &hash[..hash.len().min(2)];
        self.assets_dir().join("objects").join(prefix).join(hash)
    }
}

/// `version_id`・`assets_id` をパスの1コンポーネントとして安全に使えるよう検証する。
///
/// これらの値はMojangの公式バージョンマニフェスト由来のことが多いが、
/// Forge/NeoForgeの`predicted_installer_version_id`はTRAiNサーバー設定
/// (`minecraft_version`、管理者が設定するサーバー個別設定値)を組み込んで生成され、
/// Fabric/Quiltの場合はローダーメタAPI(Mojang以外の第三者サービス)が返す`id`
/// フィールドをそのまま信頼している。いずれも呼び出し元の外側にある入力のため、
/// ディレクトリ区切り文字(`/`・`\`)や`..`(親ディレクトリ参照)が混入していた場合、
/// そのまま`Path::join`すると`root`の外側への読み書き(パストラバーサル)を
/// 許してしまう。安全でない場合は固定の代替名を返す(以降の処理は通常の
/// ファイル未検出エラーとして扱われるため、追加のResult型は不要)。
fn safe_path_component(value: &str) -> &str {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\']) {
        "_invalid_"
    } else {
        value
    }
}

/// TRAiN Launcher自身の設定ディレクトリ (`%APPDATA%\train-launcher` 等)。
///
/// プロファイル設定 (`profile.rs`) はこの直下の `profiles.json` に保存する。
/// ゲーム本体のファイル(バージョンJar・ライブラリ・アセット)は
/// [`default_minecraft_root`] を参照すること。
pub fn default_launcher_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("train-launcher")
}

/// TRAiN管理プロファイル(`ProfileSource::Train`)専用の、隔離されたゲームディレクトリ
/// (Mod・リソースパック・セーブデータ等の実際の保存先)。
///
/// [`crate::profile::Profile::game_dir`] が未指定のTRAiN管理プロファイルに対して、
/// 全プロファイル共通の `.minecraft` の代わりに割り当てる既定値
/// ([`crate::profile::ensure_isolated_game_dir`]参照)。Minecraftのバージョン・
/// Modローダーが異なるプロファイル同士でMod・リソースパックが混在して動作しなくなることを
/// 防ぐため、プロファイルごとに専用のフォルダを使う。
///
/// `profile_id` がそのまま安全な単一パス要素として使える場合(区切り文字や `..` を
/// 含まない通常のUUID・`train-<server_id>` 形式等)はそれをそのままフォルダ名に使う。
/// そうでない場合、[`safe_path_component`]のように固定の代替名(`_invalid_`)へ
/// 丸めてしまうと、区切り文字を含む異なる複数のプロファイルIDが同じフォルダに
/// 割り当てられてしまい、それらのプロファイル間でMod・リソースパック・セーブデータが
/// 混在してしまう(隔離が壊れる)。これを防ぐため、安全な単一パス要素にできない場合は
/// `profile_id` のバイト列をそのまま16進数エンコードし、常に一意な単一パス要素になる
/// ようにする。
pub fn profile_game_dir(profile_id: &str) -> PathBuf {
    let is_safe_component = !profile_id.is_empty()
        && profile_id != "."
        && profile_id != ".."
        && !profile_id.contains(['/', '\\']);
    let component = if is_safe_component {
        profile_id.to_string()
    } else {
        let encoded: String = profile_id
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("id-{encoded}")
    };
    default_launcher_root().join("profiles").join(component)
}

/// 公式Minecraft Launcherが使用するゲームデータディレクトリ(`.minecraft` 相当)。
///
/// TRAiN Launcherはバージョンjar・ライブラリ・アセットを公式Launcherと共有する設計とし、
/// 二重ダウンロードやディスク容量の浪費を避ける(ユーザーの明示的な選択による)。
/// このためTRAiN Launcher・公式Launcherのどちらでダウンロード/起動しても、
/// 互いに同じファイル群を再利用できる。
pub fn default_minecraft_root() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".minecraft")
    }
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("minecraft")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".minecraft")
    }
}

/// 実際に使用するゲームディレクトリを解決する。
///
/// `override_dir` (設定画面で指定された [`crate::settings::AppSettings::game_directory`])
/// が指定されていればそれを、未指定または空文字列の場合は [`default_minecraft_root`] を返す。
/// この上書き先を使う場合、公式Minecraft Launcherとのファイル共有・
/// `launcher_profiles.json` 連携は行われなくなる点に注意(上書き先は独自ディレクトリのため)。
pub fn effective_minecraft_root(override_dir: Option<&str>) -> PathBuf {
    match override_dir.map(str::trim) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => default_minecraft_root(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_dir_rejects_path_traversal() {
        let paths = LauncherPaths::new(PathBuf::from("C:/minecraft"));
        let dir = paths.version_dir("../../evil");
        assert!(!dir.to_string_lossy().contains(".."));
        assert_eq!(dir, paths.versions_dir().join("_invalid_"));
    }

    #[test]
    fn version_json_path_rejects_path_traversal() {
        let paths = LauncherPaths::new(PathBuf::from("C:/minecraft"));
        let path = paths.version_json_path("..\\..\\evil");
        assert!(path.starts_with(paths.versions_dir()));
        assert!(!path.to_string_lossy().contains(".."));
    }

    #[test]
    fn version_json_path_keeps_normal_ids_unchanged() {
        let paths = LauncherPaths::new(PathBuf::from("C:/minecraft"));
        let path = paths.version_json_path("1.20.1-forge-47.2.0");
        assert_eq!(
            path,
            paths
                .versions_dir()
                .join("1.20.1-forge-47.2.0")
                .join("1.20.1-forge-47.2.0.json")
        );
    }

    #[test]
    fn asset_index_path_rejects_path_traversal() {
        let paths = LauncherPaths::new(PathBuf::from("C:/minecraft"));
        let path = paths.asset_index_path("../../evil");
        assert!(path.starts_with(paths.assets_dir()));
        assert!(!path.to_string_lossy().contains(".."));
    }

    #[test]
    fn profile_game_dir_rejects_path_traversal() {
        let dir = profile_game_dir("../../evil");
        assert!(dir.starts_with(default_launcher_root().join("profiles")));
        assert!(!dir.to_string_lossy().contains(".."));
    }

    #[test]
    fn profile_game_dir_keeps_normal_ids_unchanged() {
        let dir = profile_game_dir("train-abc123");
        assert_eq!(
            dir,
            default_launcher_root().join("profiles").join("train-abc123")
        );
    }

    #[test]
    fn profile_game_dir_disambiguates_different_unsafe_ids() {
        // 区切り文字を含む(安全な単一パス要素にできない)異なるIDが、固定の
        // フォールバック名(`_invalid_`)へ丸められて同じフォルダに衝突しないこと。
        let dir_a = profile_game_dir("train/a");
        let dir_b = profile_game_dir("train/b");
        assert_ne!(dir_a, dir_b);
    }

    #[test]
    fn profile_game_dir_is_deterministic_for_unsafe_ids() {
        let dir_a = profile_game_dir("train/a");
        let dir_b = profile_game_dir("train/a");
        assert_eq!(dir_a, dir_b);
    }
}


//! プロファイル(起動構成)管理。
//!
//! プロファイルは「どのMinecraftバージョンを・どのModローダーで・どのサーバーに接続して
//! 起動するか」をまとめた設定単位。TRAiNの所属サーバーから自動取得した設定、または
//! ユーザーが手動で作成した設定のいずれからも生成できる想定。
//!
//! 保存先は `paths::default_launcher_root()` 直下の `profiles.json` (単純なJSON配列)。
//!
//! `.minecraft` フォルダは公式Minecraft Launcherと共有しているため
//! ([`crate::paths::default_minecraft_root`])、プロファイルについても双方向に連携する:
//! - TRAiN Launcherで作成/更新/削除したプロファイルは、公式ランチャーの
//!   `launcher_profiles.json` にも反映し、公式ランチャー側の起動構成一覧にも表示されるようにする。
//! - 逆に公式ランチャー側で作成済みのプロファイルも [`list_profiles`] の結果に取り込み、
//!   TRAiN Launcher側でも表示・編集・起動できるようにする([`ProfileSource::Official`])。
//! 公式ランチャー側との連携(`launcher_profiles.json`の読み書き)に失敗しても、TRAiN側の
//! `profiles.json` への保存自体は成功させる(標準エラー出力にログを残すのみに留める)。
//!
//! Mod・リソースパックは、TRAiN管理プロファイル(`ProfileSource::Train`)ごとに専用の
//! 隔離フォルダで管理する([`ensure_isolated_game_dir`])。Minecraftのバージョン・
//! Modローダーが異なるプロファイル同士で同じ `mods`/`resourcepacks` フォルダを共有すると、
//! 一方のMod・リソースパックがもう一方でも読み込まれてしまい起動できなくなるため。

use serde::{Deserialize, Serialize};

use crate::launcher_profiles::{self, OfficialProfile};
use crate::paths::{default_launcher_root, effective_minecraft_root};
use crate::CoreError;

/// プロファイルの管理元。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    /// TRAiN Launcherの `profiles.json` で管理されているプロファイル。
    #[default]
    Train,
    /// 公式Minecraft Launcherの `launcher_profiles.json` にのみ存在し、TRAiN側では
    /// まだ保存されていないプロファイル(表示専用の取り込みであり、編集/起動すると
    /// TRAiN側にも取り込まれ`Train`として保存される)。
    Official,
}

/// 1つの起動プロファイル。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub minecraft_version: String,
    /// Modローダーの種類(`"fabric"` / `"quilt"` / `"forge"` / `"neoforge"`、
    /// 大文字小文字は区別しない)。指定されている場合、起動時に
    /// `minecraft_version`(バニラ版)を基準にローダーを自動導入し、
    /// 導入後の派生バージョンで起動する(未対応の文字列の場合は起動時にエラーになる)。
    #[serde(default)]
    pub mod_loader: Option<String>,
    /// 導入するローダーの具体的なバージョン(例: `"0.19.5"`)。未指定の場合は
    /// 安定版の最新(Forgeは`recommended`)を自動選択する。`mod_loader` が
    /// `None` の場合は無視される。
    #[serde(default)]
    pub mod_loader_version: Option<String>,
    /// TRAiNの所属サーバーから自動取得したプロファイルの場合、そのサーバーID。
    #[serde(default)]
    pub server_id: Option<String>,
    /// このプロファイル専用のゲームディレクトリ(Mod・リソースパック・セーブデータ・
    /// `options.txt` 等の実際の保存先)。
    ///
    /// TRAiN管理プロファイル(`ProfileSource::Train`)で未指定の場合、[`create_profile`]/
    /// [`update_profile`]経由で保存された後に[`ensure_isolated_game_dir`]が
    /// [`crate::paths::profile_game_dir`]の専用フォルダを自動的に割り当てる
    /// (Minecraftのバージョン・Modローダーが異なるプロファイル同士でMod・リソースパックが
    /// 混在して動作しなくなることを防ぐため)。[`Profile::effective_game_dir`]自体は、
    /// 未指定の場合の従来通りのフォールバック(全プロファイル共通の `.minecraft` 相当
    /// ディレクトリ)も引き続きサポートする(`ProfileSource::Official`のプロファイル等)。
    ///
    /// バージョンjar・ライブラリ・アセットは指定の有無に関わらず常に共通ディレクトリ側を
    /// 再利用する(ディスク容量節約のため、これらはこのフィールドの影響を受けない)。
    /// 公式Minecraft Launcherの `launcher_profiles.json` が持つ同名の `gameDir` 概念と
    /// 対応しており、双方向に同期される。
    #[serde(default)]
    pub game_dir: Option<String>,
    /// 必要条件に適合する場合に優先するJava実行ファイル。
    /// 未指定または不適合の場合は既定設定・自動検索・自動取得の順に解決する。
    #[serde(default)]
    pub java_path: Option<String>,
    /// JVMヒープ最大値(MB)。未指定の場合は `-Xmx` を付与しない(JVM既定値に任せる)。
    #[serde(default)]
    pub max_memory_mb: Option<u32>,
    #[serde(default)]
    pub source: ProfileSource,
    /// 最終起動日時(ISO 8601、UTC)。ホーム画面の「最近使ったプロファイル」表示に使用する。
    /// 未起動の場合は `None`。
    #[serde(default)]
    pub last_launched_at: Option<String>,
    /// 直近で `servers.dat` に登録したサーバーアドレス([`crate::server_list::upsert`]参照)。
    /// 次回起動時にアドレスが変わっていた場合、同一エントリを「移動」として扱うために使う
    /// (利用者が並び替えた位置やアイコンは維持したまま、接続先だけ更新する)。
    #[serde(default)]
    pub last_server_address: Option<String>,
    /// これまでにこの仕組みで自動有効化したことがあるリソースパックの識別子
    /// (`options.txt` の `resourcePacks:` に書き込む形式そのままの `file/<ファイル名>`)。
    /// [`crate::resource_pack_options::apply`]が、利用者が自分で無効化したパックを
    /// 毎回勝手に戻さないための判定に使う。
    #[serde(default)]
    pub enabled_resource_packs: Vec<String>,
    /// TRAiNがこのプロファイルのために共有 `.minecraft/mods` へ配置したファイル名一覧。
    /// サーバー切り替え時に、このプロファイルが配置したファイルだけを安全に削除するために使う
    /// (ユーザーが手動で追加したMod・他プロファイルが配置したファイルには一切触れない)。
    /// `game_dir` の指定有無に関わらず、共有フォルダから専用フォルダへの移行が未完了の
    /// ファイルがあれば空にならない。[`ensure_isolated_game_dir`]が専用フォルダを
    /// 割り当てる際(または割り当て済みでも移行未完了のファイルが残っている場合)、
    /// ここに記録済みのファイルを共有フォルダから専用フォルダへ移動し、移動が成功した
    /// ものだけをこの一覧から取り除く(失敗したファイルは次回呼び出し時に再試行できる
    /// よう残す)。
    #[serde(default)]
    pub managed_mod_filenames: Vec<String>,
    /// 同様に `.minecraft/resourcepacks` へ配置したファイル名一覧。
    #[serde(default)]
    pub managed_resource_pack_filenames: Vec<String>,
}

impl Profile {
    /// Mod・リソースパック・セーブデータ等の実際の保存先ディレクトリを解決する。
    ///
    /// `game_dir` が指定されていればそれを、未指定(または空文字列)の場合は
    /// `shared_minecraft_root`(全プロファイル共通の `.minecraft` 相当ディレクトリ)を返す。
    /// バージョンjar・ライブラリ・アセットの解決には使わないこと
    /// (それらは常に `shared_minecraft_root` 側、[`crate::launch::build_launch_command`]
    /// 参照)。
    pub fn effective_game_dir(&self, shared_minecraft_root: &std::path::Path) -> std::path::PathBuf {
        match self.game_dir.as_deref().map(str::trim) {
            Some(dir) if !dir.is_empty() => std::path::PathBuf::from(dir),
            _ => shared_minecraft_root.to_path_buf(),
        }
    }
}

fn profiles_file_path() -> std::path::PathBuf {
    default_launcher_root().join("profiles.json")
}

/// 設定で上書きされていればその値を、なければ既定の `.minecraft` 相当ディレクトリを返す。
fn minecraft_root() -> std::path::PathBuf {
    let settings = crate::settings::load_settings().unwrap_or_default();
    effective_minecraft_root(settings.game_directory.as_deref())
}

fn load_all() -> Result<Vec<Profile>, CoreError> {
    match std::fs::read(profiles_file_path()) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err.into()),
    }
}

fn save_all(profiles: &[Profile]) -> Result<(), CoreError> {
    use std::io::Write;

    let path = profiles_file_path();
    let parent = default_launcher_root();
    std::fs::create_dir_all(&parent)?;
    let mut file = tempfile::Builder::new().prefix(".profiles-").tempfile_in(&parent)?;
    file.write_all(&serde_json::to_vec_pretty(profiles)?)?;
    file.as_file().sync_all()?;
    file.persist(&path).map_err(|err| err.error)?;
    Ok(())
}

/// `javaArgs` 文字列から `-Xmx` 値をMB単位で抽出する(見つからない/解釈できない場合は `None`)。
/// 例: `"-Xmx4096M -Xms4096M"` → `Some(4096)`、`"-Xmx4G"` → `Some(4096)`。
fn parse_max_memory_mb(java_args: &str) -> Option<u32> {
    for token in java_args.split_whitespace() {
        if let Some(rest) = token.strip_prefix("-Xmx") {
            if rest.is_empty() {
                continue;
            }
            let (digits, unit) = rest.split_at(rest.len() - 1);
            if let Ok(value) = digits.parse::<u32>() {
                match unit.to_ascii_uppercase().as_str() {
                    "M" => return Some(value),
                    "G" => return Some(value.saturating_mul(1024)),
                    _ => {}
                }
            }
        }
    }
    None
}

fn official_to_profile(official: OfficialProfile) -> Profile {
    let max_memory_mb = official.java_args.as_deref().and_then(parse_max_memory_mb);
    Profile {
        id: official.id,
        name: official.name,
        minecraft_version: official.last_version_id.unwrap_or_default(),
        mod_loader: None,
        mod_loader_version: None,
        server_id: None,
        game_dir: official.game_dir,
        java_path: official.java_dir,
        max_memory_mb,
        source: ProfileSource::Official,
        last_launched_at: None,
        last_server_address: None,
        enabled_resource_packs: Vec::new(),
        managed_mod_filenames: Vec::new(),
        managed_resource_pack_filenames: Vec::new(),
    }
}

/// TRAiN Launcherのプロファイルを公式ランチャーの `launcher_profiles.json` にも反映する。
/// 失敗してもTRAiN側の保存自体は失敗させたくないため、エラーはログ出力のみに留める。
fn sync_to_official_launcher(profile: &Profile) {
    if let Err(err) = launcher_profiles::upsert_profile(&minecraft_root(), profile) {
        eprintln!("failed to sync profile to launcher_profiles.json: {err}");
    }
}

/// 保存済みプロファイル一覧を取得する。
///
/// TRAiN Launcher自身の `profiles.json` に加えて、公式ランチャーの
/// `launcher_profiles.json` に存在し、まだTRAiN側に取り込まれていないプロファイルも
/// (`ProfileSource::Official` として)一覧に含める。
pub fn list_profiles() -> Result<Vec<Profile>, CoreError> {
    let mut profiles = load_all()?;
    let known_ids: std::collections::HashSet<String> =
        profiles.iter().map(|profile| profile.id.clone()).collect();

    for official in launcher_profiles::read_all(&minecraft_root())? {
        if !known_ids.contains(&official.id) {
            profiles.push(official_to_profile(official));
        }
    }
    Ok(profiles)
}

/// 新規プロファイルを作成する。同じIDが既に存在する場合はエラーを返す。
pub fn create_profile(profile: Profile) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    if profiles.iter().any(|existing| existing.id == profile.id) {
        return Err(CoreError::ProfileAlreadyExists(profile.id));
    }
    profiles.push(profile.clone());
    save_all(&profiles)?;
    sync_to_official_launcher(&profile);
    Ok(())
}

/// IDを指定して1件のプロファイルを取得する。TRAiN側に無い場合は公式ランチャー側
/// (`launcher_profiles.json`)も参照する。どちらにも存在しない場合はエラーを返す。
pub fn get_profile(id: &str) -> Result<Profile, CoreError> {
    if let Some(profile) = load_all()?.into_iter().find(|profile| profile.id == id) {
        return Ok(profile);
    }
    let official = launcher_profiles::read_all(&minecraft_root())?
        .into_iter()
        .find(|official| official.id == id);
    match official {
        Some(official) => Ok(official_to_profile(official)),
        None => Err(CoreError::ProfileNotFound(id.to_string())),
    }
}

/// 既存プロファイルを更新する(IDは変更不可)。
///
/// 対象IDがTRAiN側の `profiles.json` にまだ存在しない場合(公式ランチャー側のみに
/// 存在するプロファイルをTRAiN側で編集した場合など)は、新規追加として取り込む。
pub fn update_profile(profile: Profile) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    match profiles.iter().position(|existing| existing.id == profile.id) {
        Some(index) => profiles[index] = profile.clone(),
        None => profiles.push(profile.clone()),
    }
    save_all(&profiles)?;
    sync_to_official_launcher(&profile);
    Ok(())
}

/// Restore game-file bookkeeping without changing launch preferences or the
/// official launcher's profile file.
pub fn save_game_state(profile: &Profile) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    let index = match profiles.iter().position(|current| current.id == profile.id) {
        Some(index) => index,
        None => {
            profiles.push(get_profile(&profile.id)?);
            profiles.len() - 1
        }
    };
    copy_game_state(&mut profiles[index], profile);
    save_all(&profiles)
}

fn copy_game_state(target: &mut Profile, source: &Profile) {
    target.managed_mod_filenames.clone_from(&source.managed_mod_filenames);
    target.managed_resource_pack_filenames.clone_from(&source.managed_resource_pack_filenames);
    target.enabled_resource_packs.clone_from(&source.enabled_resource_packs);
    target.last_server_address.clone_from(&source.last_server_address);
}

/// プロファイルを削除する。TRAiN側・公式ランチャー側のどちらか一方にのみ存在する場合も
/// 削除する。どちらにも存在しない場合はエラーを返す。
pub fn delete_profile(id: &str) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    let original_len = profiles.len();
    profiles.retain(|profile| profile.id != id);
    let existed_in_train_store = profiles.len() != original_len;
    if existed_in_train_store {
        save_all(&profiles)?;
    }

    let removed_from_official = launcher_profiles::remove_profile(&minecraft_root(), id)
        .unwrap_or_else(|err| {
            eprintln!("failed to remove profile from launcher_profiles.json: {err}");
            false
        });

    if !existed_in_train_store && !removed_from_official {
        return Err(CoreError::ProfileNotFound(id.to_string()));
    }
    Ok(())
}

/// TRAiN管理プロファイル(`ProfileSource::Train`)に、専用の隔離ゲームディレクトリを
/// 割り当てる(`game_dir` が未指定の場合のみ)。
///
/// 元々Mod・リソースパックは全プロファイル共通の `.minecraft` フォルダで管理していたが、
/// Minecraftのバージョン・Modローダーが異なるプロファイル同士でMod・リソースパックが
/// 混在すると起動できなくなる問題があったため、`game_dir` が未指定(=共通フォルダを使う
/// 設定)のTRAiN管理プロファイルを見つけ次第、[`crate::paths::profile_game_dir`] が返す
/// 専用フォルダを割り当てる。
///
/// フォルダの割り当て(`launcher_profiles.json`への同期)より前に、必ず実際の
/// ディレクトリ(`mods`/`resourcepacks`)をディスク上に作成する。以前このタイミングが
/// 逆(ディレクトリ未作成のままプロファイルを同期)だったため、公式Minecraft Launcherが
/// `gameDir` の実体が存在しないプロファイルを一覧に表示しない問題が発生していた。
///
/// このプロファイルが既に管理していたファイル(`managed_mod_filenames`/
/// `managed_resource_pack_filenames`)があれば、共有フォルダから新しい専用フォルダへ
/// 移動する(移動元に存在しない場合は無視する)。共有フォルダ内の、このプロファイルが
/// 管理していないファイル(手動で追加したMod・他プロファイルのファイル)には一切触れない
/// (どのプロファイルのものか区別できないため。該当プロファイルでのみ再導入が必要)。
/// 移動に失敗したファイル(移行先に同名のファイル・ディレクトリが既に存在する等)は
/// `managed_mod_filenames`/`managed_resource_pack_filenames` に残したままにし、
/// 専用フォルダの割り当て自体は先に確定させる(公式ランチャーでの表示問題を再発させない
/// ため)。残った未移行ファイルは、次回この関数が呼ばれた際(起動・プロファイル保存・
/// サーバー再参加時など)に再試行される。
///
/// `ProfileSource::Official` のプロファイルは何もせずそのまま返す。既に `game_dir` が
/// 指定済み(空文字列を除く)のプロファイルは、未移行ファイルが残っていなければそのまま
/// 返す。専用フォルダの割り当て・移動を行った場合は [`update_profile`] で永続化・
/// 公式ランチャーへの同期まで行う。
pub fn ensure_isolated_game_dir(id: &str) -> Result<Profile, CoreError> {
    let mut profile = get_profile(id)?;
    if profile.source != ProfileSource::Train {
        return Ok(profile);
    }
    let already_set = profile
        .game_dir
        .as_deref()
        .map(str::trim)
        .map(|dir| !dir.is_empty())
        .unwrap_or(false);
    let has_pending_migrations = !profile.managed_mod_filenames.is_empty()
        || !profile.managed_resource_pack_filenames.is_empty();
    if already_set && !has_pending_migrations {
        return Ok(profile);
    }

    let shared_root = minecraft_root();
    let target_dir = if already_set {
        // 既にユーザー独自の`game_dir`が設定されている場合はそれを尊重する
        // (未移行ファイルの再試行のみを行う)。
        std::path::PathBuf::from(profile.game_dir.as_deref().unwrap_or_default().trim())
    } else {
        crate::paths::profile_game_dir(&profile.id)
    };
    std::fs::create_dir_all(target_dir.join("mods"))?;
    std::fs::create_dir_all(target_dir.join("resourcepacks"))?;

    profile.managed_mod_filenames = migrate_managed_files(
        &profile.managed_mod_filenames,
        &shared_root.join("mods"),
        &target_dir.join("mods"),
    );
    profile.managed_resource_pack_filenames = migrate_managed_files(
        &profile.managed_resource_pack_filenames,
        &shared_root.join("resourcepacks"),
        &target_dir.join("resourcepacks"),
    );

    profile.game_dir = Some(target_dir.display().to_string());
    update_profile(profile.clone())?;
    Ok(profile)
}

/// `filenames` の各ファイルを `from_dir` から `to_dir` へ移動する。移動元に存在しない
/// ファイルは無視する(既に手動削除されている・元々存在しなかった等)。
///
/// 戻り値は、まだ移行できていない(=引き続き追跡が必要な)ファイル名一覧。移動に失敗
/// した場合(標準エラー出力へログを出力した上で)そのファイル名を戻り値に含め、
/// 呼び出し元(`ensure_isolated_game_dir`)が次回以降も再試行できるようにする。
/// 移動元に存在しなかったファイル、パストラバーサル等で不正と判断したファイルは
/// (再試行しても解決しないため)戻り値に含めない。
///
/// `filenames` は永続化された`profiles.json`由来であり、外部(悪意ある編集・過去の
/// 不具合等)から不正な値が混入している可能性を排除できないため、`from_dir`/`to_dir`
/// への結合前に単一の安全なパス要素であることを検証する(パス区切り文字・`..`・
/// 絶対パスを含む場合は無視し、`mods`/`resourcepacks` の外側のファイルには一切
/// アクセスしない)。
fn migrate_managed_files(
    filenames: &[String],
    from_dir: &std::path::Path,
    to_dir: &std::path::Path,
) -> Vec<String> {
    let mut pending = Vec::new();
    for filename in filenames {
        let path = std::path::Path::new(filename);
        let is_safe_component = !filename.is_empty()
            && filename != ".."
            && !filename.contains('/')
            && !filename.contains('\\')
            && !path.is_absolute()
            && path.file_name().is_some();
        if !is_safe_component {
            // 安全な単一パス要素ではないファイル名は、次回再試行しても解決しないため
            // 追跡対象から外す(移動もしない)。
            continue;
        }

        let src = from_dir.join(filename);
        if !src.exists() {
            continue;
        }
        if let Err(err) = std::fs::rename(&src, to_dir.join(filename)) {
            eprintln!("failed to migrate managed file {filename:?} to isolated profile dir: {err}");
            pending.push(filename.clone());
        }
    }
    pending
}

/// 指定プロファイルの最終起動日時を現在時刻(UTC)で更新する。
///
/// ホーム画面の「最近使ったプロファイル」表示のために、`launch_minecraft`/
/// `join_train_server` からの起動成功時に呼び出される。対象IDがTRAiN側の
/// `profiles.json` にまだ存在しない場合(公式ランチャー側のみに存在するプロファイルを
/// 起動した場合など)は、[`get_profile`] で解決した内容を新規に取り込んで保存する。
pub fn mark_launched(id: &str) -> Result<(), CoreError> {
    let mut profiles = load_all()?;
    let now = chrono::Utc::now().to_rfc3339();
    match profiles.iter().position(|profile| profile.id == id) {
        Some(index) => profiles[index].last_launched_at = Some(now),
        None => {
            let mut profile = get_profile(id)?;
            profile.last_launched_at = Some(now);
            profiles.push(profile);
        }
    }
    save_all(&profiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn restored_game_state_does_not_replace_launch_preferences() {
        let mut target = sample_profile(Some("target-game"));
        target.java_path = Some("chosen-java".to_string());
        target.max_memory_mb = Some(8192);
        let mut source = sample_profile(Some("other-game"));
        source.id = "other-profile".to_string();
        source.name = "other-name".to_string();
        source.minecraft_version = "other-version".to_string();
        source.managed_mod_filenames = vec!["restored-mod.jar".to_string()];
        source.enabled_resource_packs = vec!["file/restored-pack.zip".to_string()];
        copy_game_state(&mut target, &source);
        assert_eq!(target.id, "test");
        assert_eq!(target.name, "test");
        assert_eq!(target.minecraft_version, "1.20.4");
        assert_eq!(target.game_dir.as_deref(), Some("target-game"));
        assert_eq!(target.java_path.as_deref(), Some("chosen-java"));
        assert_eq!(target.max_memory_mb, Some(8192));
        assert_eq!(target.managed_mod_filenames, source.managed_mod_filenames);
        assert_eq!(target.enabled_resource_packs, source.enabled_resource_packs);
    }

    fn sample_profile(game_dir: Option<&str>) -> Profile {
        Profile {
            id: "test".to_string(),
            name: "test".to_string(),
            minecraft_version: "1.20.4".to_string(),
            mod_loader: None,
            mod_loader_version: None,
            server_id: None,
            game_dir: game_dir.map(str::to_string),
            java_path: None,
            max_memory_mb: None,
            source: ProfileSource::Train,
            last_launched_at: None,
            last_server_address: None,
            enabled_resource_packs: Vec::new(),
            managed_mod_filenames: Vec::new(),
            managed_resource_pack_filenames: Vec::new(),
        }
    }

    #[test]
    fn effective_game_dir_falls_back_to_shared_root_when_unset() {
        let profile = sample_profile(None);
        let shared_root = Path::new("/shared/.minecraft");
        assert_eq!(profile.effective_game_dir(shared_root), shared_root);
    }

    #[test]
    fn effective_game_dir_falls_back_to_shared_root_when_blank() {
        let profile = sample_profile(Some("   "));
        let shared_root = Path::new("/shared/.minecraft");
        assert_eq!(profile.effective_game_dir(shared_root), shared_root);
    }

    #[test]
    fn effective_game_dir_uses_override_when_set() {
        let profile = sample_profile(Some("/custom/profile-dir"));
        let shared_root = Path::new("/shared/.minecraft");
        assert_eq!(
            profile.effective_game_dir(shared_root),
            Path::new("/custom/profile-dir")
        );
    }

    #[test]
    fn migrate_managed_files_moves_tracked_files_only() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        std::fs::write(from.path().join("tracked.jar"), b"tracked").unwrap();
        std::fs::write(from.path().join("untracked.jar"), b"untracked").unwrap();

        let pending = migrate_managed_files(
            &["tracked.jar".to_string()],
            from.path(),
            to.path(),
        );

        assert!(pending.is_empty());
        assert!(!from.path().join("tracked.jar").exists());
        assert!(to.path().join("tracked.jar").exists());
        // 管理対象外のファイルには一切触れない(他プロファイル・手動追加のMod等)。
        assert!(from.path().join("untracked.jar").exists());
        assert!(!to.path().join("untracked.jar").exists());
    }

    #[test]
    fn migrate_managed_files_silently_skips_missing_source_files() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();

        // パニックせず、移動先にも作られず、再試行対象にも残らないことを確認する。
        let pending =
            migrate_managed_files(&["already-deleted.jar".to_string()], from.path(), to.path());

        assert!(pending.is_empty());
        assert!(!to.path().join("already-deleted.jar").exists());
    }

    #[test]
    fn migrate_managed_files_ignores_path_traversal_filenames() {
        let base = tempfile::tempdir().unwrap();
        let from = base.path().join("from");
        let to = base.path().join("to");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        // `from`の外側(隔離フォルダの外)に、移動されると困るファイルを置いておく。
        std::fs::write(base.path().join("outside.jar"), b"secret").unwrap();

        let pending = migrate_managed_files(&["../outside.jar".to_string()], &from, &to);

        // 不正なファイル名は再試行しても解決しないため、追跡対象からも外れる。
        assert!(pending.is_empty());
        // `from`の外側のファイルには一切手を付けていないこと(パストラバーサル対策)。
        assert!(base.path().join("outside.jar").exists());
        assert!(!to.join("outside.jar").exists());
    }

    #[test]
    fn migrate_managed_files_keeps_failed_renames_pending_for_retry() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        std::fs::write(from.path().join("conflict.jar"), b"data").unwrap();
        // 移動先に同名のディレクトリを作っておき、rename が必ず失敗するようにする。
        std::fs::create_dir(to.path().join("conflict.jar")).unwrap();

        let pending = migrate_managed_files(&["conflict.jar".to_string()], from.path(), to.path());

        assert_eq!(pending, vec!["conflict.jar".to_string()]);
        // 移動に失敗したファイルは移行元に残ったままになる(次回呼び出し時に再試行できる)。
        assert!(from.path().join("conflict.jar").exists());
    }
}

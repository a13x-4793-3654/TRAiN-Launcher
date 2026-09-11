//! マルチプレイの「サーバーを選択」一覧(`servers.dat`)へ、TRAiN管理サーバーを自動登録する。
//!
//! これが無いと、Mod・リソースパックの導入までは自動で終わっていても、利用者は結局
//! 手動で「サーバーを追加」してアドレスを入力する必要があり、「起動はできたがサーバー設定が
//! 何も反映されていない」という体験になる。
//!
//! [TRAiN-Setup](https://github.com/a13x-4793-3654/TRAiN-Setup)の
//! `TrainClientSetup/ServerList.cs` と同じ設計を踏襲している:
//! - 利用者が自分で追加した他の登録には一切触れない。
//! - 同じアドレスの登録が既にあれば、名前だけ最新化する(重複追加しない)。
//! - 接続先アドレスが変わった場合(このツールが前回登録したアドレスが分かっている場合)は、
//!   古い登録を消さずに付け替える(重複して並ぶと、利用者がどちらを使えばよいか迷う)。
//! - 1.19以降のMinecraft本体は、利用者が「サーバーへ直接接続」(Direct Connect)を使うと
//!   マルチプレイ一覧には表示しない`hidden`バイトタグ付きの登録を自動生成することがある。
//!   同じアドレスへ登録する際は、名前が変わっていなくてもこのタグを必ず解除する
//!   (これを見逃すと、`servers.dat`上は正しく登録されているのに利用者の画面には
//!   何も表示されない、という気付きにくい不具合になる)。

use std::path::Path;

use crate::nbt::{self, Compound, Tag, TagType};
use crate::CoreError;

const FILE_NAME: &str = "servers.dat";

/// [`upsert`]の結果。呼び出し側でログ・進捗表示に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertResult {
    /// 既に同じ名前・アドレスの登録があり、変更不要だった。
    Unchanged,
    /// 新規に登録を追加した。
    Added,
    /// 同じアドレスの登録があり、名前だけ更新した。
    Renamed,
    /// 接続先アドレスが変わったため、前回このツールが登録した内容を付け替えた。
    Moved,
    /// 名前・アドレスは同じだが、`hidden`(マルチプレイ一覧に表示しないフラグ)が
    /// 立っていたため解除した。
    Unhidden,
}

/// 指定のサーバーを`servers.dat`へ登録(または更新)する。
///
/// `previous_address`には、前回このツールが同じプロファイルで登録したアドレスを渡す
/// (プロファイルの永続化状態から取得、[`crate::profile::Profile::last_server_address`]参照)。
/// 初回登録時は`None`でよい。
///
/// ファイルが存在しない、または壊れていて読み取れない場合は、壊れたファイルを
/// `servers.dat.bak-<UNIXタイムスタンプ>`として同じディレクトリに退避したうえで、
/// 空の一覧から作り直す(起動全体を止めないため)。
///
/// 1.19以降、Minecraft本体は「サーバーへ直接接続」(Direct Connect)を使った際に、
/// マルチプレイ一覧には表示しない `hidden` バイトタグ付きのエントリを`servers.dat`へ
/// 自動生成することがある。同じアドレスへこの関数で登録する際は、たとえ名前が
/// 変わっていなくても、このタグを必ず`0`(表示)へ戻す。ここを見逃すと、ファイル上は
/// 正しく登録されているのに利用者のマルチプレイ画面には何も表示されない、という
/// 分かりにくい不具合になる。
pub fn upsert(
    game_dir: &Path,
    name: &str,
    address: &str,
    previous_address: Option<&str>,
) -> Result<UpsertResult, CoreError> {
    let path = game_dir.join(FILE_NAME);

    let mut root = match std::fs::read(&path) {
        Ok(bytes) if bytes.is_empty() => Compound::new(),
        Ok(bytes) => match nbt::read(&mut bytes.as_slice()) {
            Ok(root) => root,
            Err(_) => {
                backup_corrupt_file(&path)?;
                Compound::new()
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(game_dir)?;
            Compound::new()
        }
        Err(err) => return Err(err.into()),
    };

    let mut servers: Vec<Tag> = match root.get("servers") {
        Some(Tag::List(TagType::Compound, items)) => items.clone(),
        _ => Vec::new(),
    };

    // 既に同じアドレスの登録があれば、名前を最新化し、"hidden"を必ず解除する
    // (重複追加はしない)。
    for item in servers.iter_mut() {
        let Tag::Compound(entries) = item else {
            continue;
        };
        let entry = Compound(entries.clone());
        let Some(ip) = entry.get_string("ip") else {
            continue;
        };
        if !ip.eq_ignore_ascii_case(address) {
            continue;
        }
        let name_matches = entry.get_string("name") == Some(name);
        let is_hidden = matches!(entry.get("hidden"), Some(Tag::Byte(value)) if *value != 0);
        if name_matches && !is_hidden {
            return Ok(UpsertResult::Unchanged);
        }
        let mut updated = entry;
        updated.set("name", Tag::String(name.to_string()));
        updated.set("hidden", Tag::Byte(0));
        *item = updated.into_tag();
        root.set("servers", Tag::List(TagType::Compound, servers));
        write(&path, &root)?;
        return Ok(if name_matches {
            UpsertResult::Unhidden
        } else {
            UpsertResult::Renamed
        });
    }

    // 接続先が変わった場合は、前に置いた登録を付け替える。
    // 増やすと同じ名前が2つ並び、古い方を選んでしまう恐れがある。
    if let Some(previous_address) = previous_address {
        if !previous_address.eq_ignore_ascii_case(address) {
            for item in servers.iter_mut() {
                let Tag::Compound(entries) = item else {
                    continue;
                };
                let entry = Compound(entries.clone());
                let Some(ip) = entry.get_string("ip") else {
                    continue;
                };
                if !ip.eq_ignore_ascii_case(previous_address) {
                    continue;
                }
                let mut updated = entry;
                updated.set("ip", Tag::String(address.to_string()));
                updated.set("name", Tag::String(name.to_string()));
                updated.set("hidden", Tag::Byte(0));
                *item = updated.into_tag();
                root.set("servers", Tag::List(TagType::Compound, servers));
                write(&path, &root)?;
                return Ok(UpsertResult::Moved);
            }
        }
    }

    let mut added = Compound::new();
    added.set("name", Tag::String(name.to_string()));
    added.set("ip", Tag::String(address.to_string()));
    added.set("hidden", Tag::Byte(0));
    servers.push(added.into_tag());
    root.set("servers", Tag::List(TagType::Compound, servers));
    write(&path, &root)?;
    Ok(UpsertResult::Added)
}

fn write(path: &Path, root: &Compound) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut buffer = Vec::new();
    nbt::write(&mut buffer, root)?;
    // 書き込み途中でのクラッシュにより壊れたファイルが残らないよう、一時ファイルへ書いてから
    // 置き換える(`std::fs::rename`は同一ファイルシステム内であれば既存ファイルを上書きできる)。
    let tmp_path = path.with_extension("dat.tmp");
    std::fs::write(&tmp_path, &buffer)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

fn backup_corrupt_file(path: &Path) -> Result<(), CoreError> {
    if !path.exists() {
        return Ok(());
    }
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let backup_path = path.with_extension(format!("dat.bak-{timestamp}"));
    std::fs::rename(path, backup_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn adds_new_entry_when_file_does_not_exist() {
        let dir = tempdir().unwrap();
        let result = upsert(dir.path(), "碓氷鯖", "mc.example.com:25565", None).unwrap();
        assert_eq!(result, UpsertResult::Added);

        let bytes = std::fs::read(dir.path().join("servers.dat")).unwrap();
        let root = nbt::read(&mut bytes.as_slice()).unwrap();
        let Some(Tag::List(_, items)) = root.get("servers") else {
            panic!("servers missing");
        };
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn is_unchanged_when_same_name_and_address_already_registered() {
        let dir = tempdir().unwrap();
        upsert(dir.path(), "碓氷鯖", "mc.example.com:25565", None).unwrap();
        let result = upsert(dir.path(), "碓氷鯖", "mc.example.com:25565", None).unwrap();
        assert_eq!(result, UpsertResult::Unchanged);
    }

    #[test]
    fn new_entry_is_not_hidden() {
        let dir = tempdir().unwrap();
        upsert(dir.path(), "碓氷鯖", "mc.example.com:25565", None).unwrap();

        let bytes = std::fs::read(dir.path().join("servers.dat")).unwrap();
        let root = nbt::read(&mut bytes.as_slice()).unwrap();
        let Some(Tag::List(_, items)) = root.get("servers") else {
            panic!("servers missing");
        };
        let Tag::Compound(entries) = &items[0] else {
            panic!("not compound");
        };
        assert_eq!(
            Compound(entries.clone()).get("hidden"),
            Some(&Tag::Byte(0))
        );
    }

    /// Minecraft本体が「サーバーへ直接接続」(Direct Connect)時に自動生成することがある
    /// `hidden: 1` 付きの登録を、同じ名前・アドレスで再登録した際に解除できることを確認する。
    /// これを見逃すと、`servers.dat`上は正しく登録されているのにマルチプレイ一覧には
    /// 何も表示されない、という不具合になる(実際にVPS環境で発生したケース)。
    #[test]
    fn unhides_entry_that_was_marked_hidden_even_when_name_unchanged() {
        let dir = tempdir().unwrap();
        let mut root = Compound::new();
        let mut entry = Compound::new();
        entry.set("name", Tag::String("碓氷鯖".to_string()));
        entry.set("ip", Tag::String("mc.example.com:25565".to_string()));
        entry.set("hidden", Tag::Byte(1));
        root.set(
            "servers",
            Tag::List(TagType::Compound, vec![entry.into_tag()]),
        );
        let mut buffer = Vec::new();
        nbt::write(&mut buffer, &root).unwrap();
        std::fs::write(dir.path().join("servers.dat"), buffer).unwrap();

        let result = upsert(dir.path(), "碓氷鯖", "mc.example.com:25565", None).unwrap();
        assert_eq!(result, UpsertResult::Unhidden);

        let bytes = std::fs::read(dir.path().join("servers.dat")).unwrap();
        let root = nbt::read(&mut bytes.as_slice()).unwrap();
        let Some(Tag::List(_, items)) = root.get("servers") else {
            panic!("servers missing");
        };
        assert_eq!(items.len(), 1);
        let Tag::Compound(entries) = &items[0] else {
            panic!("not compound");
        };
        assert_eq!(
            Compound(entries.clone()).get("hidden"),
            Some(&Tag::Byte(0))
        );
    }

    #[test]
    fn renames_when_address_matches_but_name_changed() {
        let dir = tempdir().unwrap();
        upsert(dir.path(), "旧名前", "mc.example.com:25565", None).unwrap();
        let result = upsert(dir.path(), "新名前", "mc.example.com:25565", None).unwrap();
        assert_eq!(result, UpsertResult::Renamed);

        let bytes = std::fs::read(dir.path().join("servers.dat")).unwrap();
        let root = nbt::read(&mut bytes.as_slice()).unwrap();
        let Some(Tag::List(_, items)) = root.get("servers") else {
            panic!("servers missing");
        };
        assert_eq!(items.len(), 1);
        let Tag::Compound(entries) = &items[0] else {
            panic!("not compound");
        };
        assert_eq!(Compound(entries.clone()).get_string("name"), Some("新名前"));
    }

    #[test]
    fn moves_previous_registration_when_address_changes() {
        let dir = tempdir().unwrap();
        upsert(dir.path(), "碓氷鯖", "old.example.com:25565", None).unwrap();
        let result = upsert(
            dir.path(),
            "碓氷鯖",
            "new.example.com:25565",
            Some("old.example.com:25565"),
        )
        .unwrap();
        assert_eq!(result, UpsertResult::Moved);

        let bytes = std::fs::read(dir.path().join("servers.dat")).unwrap();
        let root = nbt::read(&mut bytes.as_slice()).unwrap();
        let Some(Tag::List(_, items)) = root.get("servers") else {
            panic!("servers missing");
        };
        // 増えず、既存の1件が付け替わっていること。
        assert_eq!(items.len(), 1);
        let Tag::Compound(entries) = &items[0] else {
            panic!("not compound");
        };
        assert_eq!(
            Compound(entries.clone()).get_string("ip"),
            Some("new.example.com:25565")
        );
        assert_eq!(
            Compound(entries.clone()).get("hidden"),
            Some(&Tag::Byte(0))
        );
    }

    #[test]
    fn preserves_user_added_entries() {
        let dir = tempdir().unwrap();
        // 利用者が自分で追加した登録を模す。
        let mut root = Compound::new();
        let mut user_entry = Compound::new();
        user_entry.set("name", Tag::String("知人のサーバー".to_string()));
        user_entry.set("ip", Tag::String("friend.example.com".to_string()));
        root.set(
            "servers",
            Tag::List(TagType::Compound, vec![user_entry.into_tag()]),
        );
        let mut buffer = Vec::new();
        nbt::write(&mut buffer, &root).unwrap();
        std::fs::write(dir.path().join("servers.dat"), buffer).unwrap();

        upsert(dir.path(), "碓氷鯖", "mc.example.com:25565", None).unwrap();

        let bytes = std::fs::read(dir.path().join("servers.dat")).unwrap();
        let root = nbt::read(&mut bytes.as_slice()).unwrap();
        let Some(Tag::List(_, items)) = root.get("servers") else {
            panic!("servers missing");
        };
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn backs_up_and_recovers_from_corrupt_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("servers.dat"), b"not valid nbt").unwrap();

        let result = upsert(dir.path(), "碓氷鯖", "mc.example.com:25565", None).unwrap();
        assert_eq!(result, UpsertResult::Added);

        let backup_exists = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("servers.dat.bak-")
            });
        assert!(backup_exists, "corrupt file should have been backed up");
    }
}

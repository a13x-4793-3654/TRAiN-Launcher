use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey},
    SigningKey,
};
use serde::{Deserialize, Serialize};
use train_launcher_auth::store::TokenRecord;
use train_launcher_server_api::fallback::{FallbackStatus, FallbackTicket, PROTOCOL};
use uuid::Uuid;

#[derive(Default)]
pub struct Credentials(pub tokio::sync::Mutex<()>);

#[derive(Clone, Default)]
pub struct TicketRegistry(Arc<Mutex<HashSet<PathBuf>>>);

impl TicketRegistry {
    pub fn cleanup(&self) {
        match self.0.lock() {
            Ok(paths) => {
                for path in paths.iter() {
                    if let Err(err) = std::fs::remove_file(path) {
                        if err.kind() != std::io::ErrorKind::NotFound {
                            eprintln!(
                                "failed to clean auth fallback ticket at launcher exit: {err}"
                            );
                        }
                    }
                }
            }
            Err(_) => eprintln!("failed to lock auth fallback ticket registry at launcher exit"),
        }
    }
}

// The whole record, including the verified cached identity, stays in the OS keyring.
#[derive(Serialize, Deserialize)]
pub struct Credential {
    pub credential_id: String,
    pub expires_at: u64,
    pub mc_uuid: String,
    pub mc_name: String,
    private_key: String,
}

pub struct LaunchContext {
    pub server_id: String,
    pub discord_id: String,
    pub address: String,
    pub credential: Credential,
}

pub fn now_ms() -> Result<u64, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "システム時刻を確認してください".to_string())?
        .as_millis() as u64)
}

pub fn identity(token: &TokenRecord) -> Result<(String, String), String> {
    let uuid = token
        .uuid
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(
            "確認済みのMinecraftアカウントがありません。復旧後にMicrosoftでサインインしてください",
        )?;
    let name = token
        .display_name
        .as_deref()
        .filter(|name| valid_name(name))
        .ok_or("確認済みのMinecraftプレイヤー名がありません。復旧後に再サインインしてください")?;
    Ok((uuid.hyphenated().to_string(), name.to_string()))
}

fn valid_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn entry(server_id: &str, discord_id: &str) -> Result<keyring::Entry, String> {
    let server = Uuid::parse_str(server_id).map_err(|_| "サーバーIDが不正です")?;
    if discord_id.is_empty()
        || discord_id.len() > 20
        || !discord_id.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("Discordで再サインインしてください".into());
    }
    keyring::Entry::new(
        "train-launcher",
        &format!("auth-fallback/{server}/{discord_id}"),
    )
    .map_err(|_| "OS資格情報ストアを利用できません。平文では保存しません".into())
}

pub fn load(server: &str, discord: &str) -> Result<Option<Credential>, String> {
    match entry(server, discord)?.get_password() {
        Ok(json) => serde_json::from_str(&json)
            .map(Some)
            .map_err(|_| "障害時接続の資格情報が壊れています。復旧後に再登録してください".into()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("OS資格情報ストアを読み込めません。障害時接続を中止しました".into()),
    }
}

pub fn save(server: &str, discord: &str, credential: &Credential) -> Result<(), String> {
    let json = serde_json::to_string(credential).map_err(|_| "資格情報を保存できません")?;
    entry(server, discord)?
        .set_password(&json)
        .map_err(|_| "OS資格情報ストアに保存できません。復旧後に再登録してください".into())
}

pub fn generate(mc_uuid: String, mc_name: String) -> Result<(Credential, String), String> {
    let key = SigningKey::generate(&mut rand::rngs::OsRng);
    let private = key.to_pkcs8_der().map_err(|_| "秘密鍵を生成できません")?;
    let public = key
        .verifying_key()
        .to_public_key_der()
        .map_err(|_| "公開鍵を生成できません")?;
    Ok((
        Credential {
            credential_id: String::new(),
            expires_at: 0,
            mc_uuid,
            mc_name,
            private_key: STANDARD.encode(private.as_bytes()),
        },
        STANDARD.encode(public.as_bytes()),
    ))
}

pub fn check_status(
    status: &FallbackStatus,
    now: u64,
    require_enabled: bool,
) -> Result<(), String> {
    if status.protocol != PROTOCOL || !status.available {
        return Err(
            "障害時接続の対応Modがサーバーに登録されていません。管理者に確認してください".into(),
        );
    }
    if require_enabled && status.enabled_until <= now {
        return Err("障害時接続は管理者が有効にした時間内だけ利用できます。通常起動への自動切替は行いません".into());
    }
    Ok(())
}

pub fn check_credential(
    credential: &Credential,
    token: &TokenRecord,
    now: u64,
) -> Result<(), String> {
    let (uuid, name) = identity(token)?;
    if credential.expires_at <= now || Uuid::parse_str(&credential.credential_id).is_err() {
        return Err("事前登録がないか7日間の有効期限が切れています。Minecraft認証の復旧後に登録してください".into());
    }
    if credential.mc_uuid != uuid || credential.mc_name != name {
        return Err(
            "事前登録と現在のMinecraftアカウントが一致しません。復旧後に再登録してください".into(),
        );
    }
    let der = STANDARD
        .decode(&credential.private_key)
        .map_err(|_| "保存された秘密鍵が不正です")?;
    SigningKey::from_pkcs8_der(&der).map_err(|_| "保存された秘密鍵が不正です")?;
    Ok(())
}

pub fn check_ticket(
    ticket: &FallbackTicket,
    context: &LaunchContext,
    now: u64,
) -> Result<(), String> {
    if ticket.protocol != PROTOCOL
        || ticket.server_id != context.server_id
        || ticket.mc_uuid != context.credential.mc_uuid
        || ticket.mc_name != context.credential.mc_name
        || Uuid::parse_str(&ticket.ticket_id).is_err()
        || ticket.expires_at <= now
        || ticket.expires_at > now.saturating_add(60_000)
    {
        return Err(
            "障害時接続チケットの本人・サーバー・有効期限が不正です。接続を中止しました".into(),
        );
    }
    Ok(())
}

pub fn check_companion(game_dir: &Path, managed: &[String]) -> Result<(), String> {
    for name in managed {
        if Path::new(name).file_name().and_then(|v| v.to_str()) != Some(name.as_str()) {
            return Err("配布Modのファイル名が不正です".into());
        }
        let file = std::fs::File::open(game_dir.join("mods").join(name))
            .map_err(|_| "配布Modを読み込めません。サーバー設定を再取得してください")?;
        let mut archive = zip::ZipArchive::new(file).map_err(|_| "配布Modのjarが壊れています")?;
        let mut metadata = match archive.by_name("fabric.mod.json") {
            Ok(metadata) => metadata,
            Err(zip::result::ZipError::FileNotFound) => continue,
            Err(_) => return Err("配布Modのメタデータを読み込めません".into()),
        };
        if metadata.size() > 64 * 1024 {
            return Err("配布Modのメタデータが大きすぎます".into());
        }
        let mut bytes = Vec::new();
        metadata
            .read_to_end(&mut bytes)
            .map_err(|_| "配布Modを読み込めません")?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "配布Modのfabric.mod.jsonが不正です")?;
        if value["id"] == "train-auth-fallback" {
            if value["version"] != "1.0.0" {
                return Err(
                    "障害時接続Modはバージョン1.0.0が必要です。管理者に確認してください".into(),
                );
            }
            return Ok(());
        }
    }
    Err("配布設定にtrain-auth-fallback 1.0.0がありません。管理者から対応Modを配布してもらってください".into())
}

pub fn cached_fabric_id(root: &Path) -> Result<String, String> {
    let versions = std::fs::read_dir(root.join("versions"))
        .map_err(|_| "ゲームの準備がありません。復旧後に通常起動してください")?;
    let mut candidates = Vec::new();
    for directory in versions {
        let directory = directory.map_err(|_| "準備済みFabricを確認できません")?;
        let name = directory.file_name().to_string_lossy().into_owned();
        let Some(version) = name
            .strip_prefix("fabric-loader-")
            .and_then(|v| v.strip_suffix("-1.21.1"))
        else {
            continue;
        };
        let parts: Vec<u32> = match version.split('.').map(str::parse).collect::<Result<_, _>>() {
            Ok(parts) => parts,
            Err(_) => continue,
        };
        if parts.len() == 3
            && parts.as_slice() >= [0, 19, 5].as_slice()
            && directory.path().join(format!("{name}.json")).is_file()
        {
            candidates.push((parts, name));
        }
    }
    candidates.sort();
    candidates.pop().map(|(_, name)| name).ok_or_else(|| {
        "Fabric Loader 0.19.5以上の準備がありません。復旧後に通常起動してください".into()
    })
}

#[derive(Serialize)]
struct TicketFile<'a> {
    server_id: &'a str,
    ticket_id: &'a str,
    expires_at: u64,
    mc_uuid: &'a str,
    mc_name: &'a str,
    address: &'a str,
    private_key: &'a str,
}

pub struct PrivateTicket {
    _directory: tempfile::TempDir,
    pub path: PathBuf,
    pub expires_at: u64,
    registry: TicketRegistry,
}

impl PrivateTicket {
    pub fn create(
        ticket: &FallbackTicket,
        context: &LaunchContext,
        registry: TicketRegistry,
    ) -> Result<Self, String> {
        check_ticket(ticket, context, now_ms()?)?;
        if context.address.is_empty()
            || context.address.len() > 255
            || context.address.chars().any(char::is_whitespace)
        {
            return Err("障害時接続のサーバーアドレスが不正です".into());
        }
        let bytes = serde_json::to_vec(&TicketFile {
            server_id: &ticket.server_id,
            ticket_id: &ticket.ticket_id,
            expires_at: ticket.expires_at,
            mc_uuid: &ticket.mc_uuid,
            mc_name: &ticket.mc_name,
            address: &context.address,
            private_key: &context.credential.private_key,
        })
        .map_err(|_| "チケットファイルを作成できません")?;
        if bytes.len() > 4096 {
            return Err("チケットファイルが4096バイトを超えています".into());
        }
        let directory = tempfile::Builder::new()
            .prefix("train-auth-fallback-")
            .tempdir()
            .map_err(|_| "チケット用ディレクトリを作成できません")?;
        secure_directory(directory.path())?;
        let path = directory
            .path()
            .canonicalize()
            .map_err(|_| "チケットの絶対パスを取得できません")?;
        // Java WindowsPath rejects verbatim prefixes containing '?'. Preserve UNC semantics.
        #[cfg(windows)]
        let path = {
            let value = path.to_str().ok_or("チケットのパスをJavaへ渡せません")?;
            if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
                PathBuf::from(format!(r"\\{unc}"))
            } else if let Some(local) = value.strip_prefix(r"\\?\") {
                PathBuf::from(local)
            } else {
                path
            }
        };
        let path = path.join("ticket.json");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(&path)
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(|_| "非公開チケットファイルを書き込めません")?;
        registry
            .0
            .lock()
            .map_err(|_| "非公開チケットの管理を開始できません")?
            .insert(path.clone());
        Ok(Self {
            _directory: directory,
            path,
            expires_at: ticket.expires_at,
            registry,
        })
    }
}

impl Drop for PrivateTicket {
    fn drop(&mut self) {
        // Delete just the one file; TempDir then removes only our randomly named directory.
        if let Err(err) = std::fs::remove_file(&self.path) {
            if err.kind() != std::io::ErrorKind::NotFound {
                eprintln!("failed to remove private auth fallback ticket: {err}");
            }
            match self.registry.0.lock() {
                Ok(mut paths) => {
                    paths.remove(&self.path);
                }
                Err(_) => eprintln!("failed to release auth fallback ticket registry"),
            }
        }
    }
}

#[cfg(unix)]
fn secure_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| "チケットの所有者限定アクセス権を設定できません".into())
}

#[cfg(windows)]
fn secure_directory(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
                SE_FILE_OBJECT,
            },
            GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;OW)\0".encode_utf16().collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Protected DACL: only the owner, including inherited permissions on the ticket file.
    unsafe {
        let mut descriptor = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err("チケットの所有者限定ACLを作成できません".into());
        }
        let mut dacl = std::ptr::null_mut();
        let mut present = 0;
        let mut defaulted = 0;
        let extracted =
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
        let result = if extracted != 0 && present != 0 && !dacl.is_null() {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null_mut(),
            )
        } else {
            1
        };
        LocalFree(descriptor);
        if result != 0 {
            return Err("チケットの所有者限定ACLを設定できません。接続を中止しました".into());
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn secure_directory(_path: &Path) -> Result<(), String> {
    Err("このOSでは安全なチケット保存を利用できません".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> LaunchContext {
        let (mut credential, _) = generate(Uuid::new_v4().to_string(), "Player_1".into()).unwrap();
        credential.credential_id = Uuid::new_v4().to_string();
        credential.expires_at = now_ms().unwrap() + 100_000;
        LaunchContext {
            server_id: Uuid::new_v4().to_string(),
            discord_id: "123".into(),
            address: "Play.example.com:25565".into(),
            credential,
        }
    }

    fn ticket(context: &LaunchContext) -> FallbackTicket {
        FallbackTicket {
            ticket_id: Uuid::new_v4().to_string(),
            expires_at: now_ms().unwrap() + 30_000,
            mc_uuid: context.credential.mc_uuid.clone(),
            mc_name: context.credential.mc_name.clone(),
            server_id: context.server_id.clone(),
            protocol: PROTOCOL.into(),
        }
    }

    #[test]
    fn key_formats_round_trip() {
        let (credential, public) = generate(Uuid::new_v4().to_string(), "Player".into()).unwrap();
        let private = STANDARD.decode(credential.private_key).unwrap();
        let key = SigningKey::from_pkcs8_der(&private).unwrap();
        assert_eq!(
            STANDARD.decode(public).unwrap(),
            key.verifying_key().to_public_key_der().unwrap().as_bytes()
        );
    }

    #[test]
    fn tickets_reject_wrong_scope_identity_protocol_and_expiry() {
        let context = context();
        let now = now_ms().unwrap();
        let mut ticket = ticket(&context);
        assert!(check_ticket(&ticket, &context, now).is_ok());
        ticket.expires_at = now;
        assert!(check_ticket(&ticket, &context, now).is_err());
        ticket.expires_at = now + 60_001;
        assert!(check_ticket(&ticket, &context, now).is_err());
        ticket = self::ticket(&context);
        ticket.mc_uuid = Uuid::new_v4().to_string();
        assert!(check_ticket(&ticket, &context, now).is_err());
        ticket = self::ticket(&context);
        ticket.server_id = Uuid::new_v4().to_string();
        assert!(check_ticket(&ticket, &context, now).is_err());
        ticket = self::ticket(&context);
        ticket.protocol = "unknown".into();
        assert!(check_ticket(&ticket, &context, now).is_err());
    }

    #[test]
    fn private_ticket_is_bounded_exact_and_removed_on_drop() {
        let context = context();
        let registry = TicketRegistry::default();
        let ticket = PrivateTicket::create(&ticket(&context), &context, registry.clone()).unwrap();
        let path = ticket.path.clone();
        let parent = path.parent().unwrap().to_path_buf();
        assert!(path.is_absolute());
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() <= 4096);
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["address"], context.address);
        assert_eq!(json.as_object().unwrap().len(), 7);
        #[cfg(windows)]
        {
            assert_owner_only_acl(&parent);
            assert_owner_only_acl(&path);
            assert!(!path.to_string_lossy().starts_with(r"\\?\"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        registry.cleanup();
        assert!(!path.exists());
        drop(ticket);
        assert!(!path.exists());
        assert!(!parent.exists());
    }

    #[test]
    fn disabled_or_unknown_status_fails_closed() {
        let mut status = FallbackStatus {
            available: true,
            enabled_until: 10,
            protocol: PROTOCOL.into(),
        };
        assert!(check_status(&status, 10, true).is_err());
        assert!(check_status(&status, 10, false).is_ok());
        status.available = false;
        assert!(check_status(&status, 0, false).is_err());
        status.available = true;
        status.protocol = "unknown".into();
        assert!(check_status(&status, 0, false).is_err());
    }

    #[cfg(windows)]
    fn assert_owner_only_acl(path: &Path) {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
                    SE_FILE_OBJECT,
                },
                DACL_SECURITY_INFORMATION,
            },
        };
        unsafe {
            let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let mut descriptor = std::ptr::null_mut();
            assert_eq!(
                GetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut descriptor
                ),
                0
            );
            let mut text = std::ptr::null_mut();
            assert_ne!(
                ConvertSecurityDescriptorToStringSecurityDescriptorW(
                    descriptor,
                    1,
                    DACL_SECURITY_INFORMATION,
                    &mut text,
                    std::ptr::null_mut()
                ),
                0
            );
            let mut len = 0;
            while *text.add(len) != 0 {
                len += 1;
            }
            let sddl = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
            LocalFree(text.cast());
            LocalFree(descriptor);
            assert_eq!(sddl.matches('(').count(), 1, "{sddl}");
            assert!(sddl.contains(";;;OW)"), "{sddl}");
        }
    }

    #[test]
    fn cached_fabric_requires_compatible_prepared_release() {
        let root = tempfile::tempdir().unwrap();
        let versions = root.path().join("versions");
        std::fs::create_dir(&versions).unwrap();
        for version in ["0.19.4", "0.19.5", "0.20.0", "0.99.0-beta"] {
            let id = format!("fabric-loader-{version}-1.21.1");
            let folder = versions.join(&id);
            std::fs::create_dir(&folder).unwrap();
            std::fs::write(folder.join(format!("{id}.json")), "{}").unwrap();
        }
        assert_eq!(
            cached_fabric_id(root.path()).unwrap(),
            "fabric-loader-0.20.0-1.21.1"
        );
    }

    #[test]
    fn credential_requires_cached_verified_identity_unexpired_record_and_valid_key() {
        let mut context = context();
        let now = now_ms().unwrap();
        let mut token = TokenRecord {
            access_token: "expired-token-not-used-for-fallback".into(),
            refresh_token: None, expires_at: Some(0),
            display_name: Some(context.credential.mc_name.clone()),
            uuid: Some(context.credential.mc_uuid.replace('-', "")), user_id: None,
        };
        assert!(check_credential(&context.credential, &token, now).is_ok());
        token.uuid = None;
        assert!(check_credential(&context.credential, &token, now).is_err());
        token.uuid = Some(context.credential.mc_uuid.clone());
        token.display_name = Some("SomeoneElse".into());
        assert!(check_credential(&context.credential, &token, now).is_err());
        token.display_name = Some(context.credential.mc_name.clone());
        context.credential.expires_at = now;
        assert!(check_credential(&context.credential, &token, now).is_err());
        context.credential.expires_at = now + 1000;
        context.credential.private_key = "invalid".into();
        assert!(check_credential(&context.credential, &token, now).is_err());
    }

    #[test]
    fn companion_must_be_manifest_managed_and_exact_version() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("mods")).unwrap();
        let path = root.path().join("mods").join("companion.jar");
        let write_mod = |id: &str, version: &str| {
            let file = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(
                serde_json::json!({"id":id,"version":version})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
            zip.finish().unwrap();
        };
        write_mod("train-auth-fallback", "1.0.0");
        assert!(check_companion(root.path(), &[]).is_err());
        assert!(check_companion(root.path(), &["companion.jar".into()]).is_ok());
        write_mod("train-auth-fallback", "1.0.1");
        assert!(check_companion(root.path(), &["companion.jar".into()]).is_err());
        write_mod("unrelated", "1.0.0");
        assert!(check_companion(root.path(), &["companion.jar".into()]).is_err());
    }
}

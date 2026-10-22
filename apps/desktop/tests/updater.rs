use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde_json::json;
use tauri_plugin_updater::{Update, UpdaterExt};

#[test]
fn native_release_formats_do_not_revert_to_legacy_updater_archives() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert_eq!(config["bundle"]["createUpdaterArtifacts"], true);
    assert_eq!(config["bundle"]["targets"], "all");

    let published: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/updater-formats.json")).unwrap();
    for (target, file) in published["platforms"].as_object().unwrap() {
        let file = file.as_str().unwrap();
        if target.starts_with("windows-") {
            assert!(file.ends_with(".exe") || file.ends_with(".msi"));
        } else if target.starts_with("linux-") {
            assert!(
                file.ends_with(".AppImage") || file.ends_with(".deb") || file.ends_with(".rpm")
            );
        } else if target.starts_with("darwin-") {
            assert!(file.ends_with(".app.tar.gz"));
        } else {
            panic!("unreviewed release target");
        }
        assert!(!file.ends_with(".zip"));
    }
}

fn signed_update(target: &str, file: &str, bytes: &[u8]) -> Update {
    let key = SigningKey::generate(&mut OsRng);
    let key_id = [0_u8; 8];
    let mut public = b"Ed".to_vec();
    public.extend_from_slice(&key_id);
    public.extend_from_slice(&key.verifying_key().to_bytes());
    let public = format!(
        "untrusted comment: generated test public key\n{}",
        STANDARD.encode(public)
    );

    let mut signature = b"Ed".to_vec();
    signature.extend_from_slice(&key_id);
    signature.extend_from_slice(&key.sign(bytes).to_bytes());
    let comment = format!("timestamp:1\tfile:{file}\tversion:999.0.0");
    let mut signed_comment = signature[10..].to_vec();
    signed_comment.extend_from_slice(comment.as_bytes());
    let global = key.sign(&signed_comment);
    let signature = format!(
        "untrusted comment: generated test signature\n{}\ntrusted comment: {comment}\n{}",
        STANDARD.encode(signature),
        STANDARD.encode(global.to_bytes()),
    );
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.config_mut().plugins =
        serde_json::from_value(json!({"updater": {"pubkey": STANDARD.encode(public)}})).unwrap();
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    let updater = app
        .updater_builder()
        .target(target)
        .endpoints(vec!["https://example.invalid/update".parse().unwrap()])
        .unwrap()
        .configure_client(|_| panic!("updater regression tests must not access the network"))
        .build()
        .unwrap();
    updater
        .restore_update(json!({
            "version": "999.0.0",
            "url": format!("https://example.invalid/{file}"),
            "signature": STANDARD.encode(signature),
        }))
        .unwrap()
        .unwrap()
}

#[test]
fn signed_raw_exe_and_msi_manifests_verify_and_tampered_bytes_cannot_install() {
    for (target, file, payload) in [
        (
            "windows-x86_64-nsis",
            "synthetic-setup.exe",
            b"MZ synthetic updater test".as_slice(),
        ),
        (
            "windows-x86_64-msi",
            "synthetic.msi",
            b"\xd0\xcf\x11\xe0 synthetic updater test".as_slice(),
        ),
    ] {
        let update = signed_update(target, file, payload);
        assert_eq!(update.target, target);
        assert!(update.download_url.path().ends_with(file));
        update.verify(payload).unwrap();
        assert!(update.verify(b"tampered").is_err());
        // Never invoke a real installer: verification must reject these bytes first.
        assert!(matches!(
            update.install(b"tampered"),
            Err(tauri_plugin_updater::Error::Minisign(_))
        ));
    }
}

use super::*;
use ed25519_dalek::{Signer, SigningKey};
use std::os::unix::fs::PermissionsExt;

#[test]
fn helper_requests_authenticate_and_reject_stale_or_tampered_packages() -> anyhow::Result<()> {
    let stage = tempfile::tempdir()?;
    fs::set_permissions(stage.path(), fs::Permissions::from_mode(0o700))?;
    let key = SigningKey::from_bytes(&[42; 32]);
    let public: String = key
        .verifying_key()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let manifest = release::Manifest {
        schema: 2,
        version: "20261010.2".into(),
        assets: vec![release::Asset {
            target: "x86_64-unknown-linux-gnu".into(),
            name: "Herdr-20261010.2-x86_64-unknown-linux-gnu.deb".into(),
            size: 1,
            sha256: "00".repeat(32),
        }],
    };
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let prepared = Prepared {
        stage,
        package: Package {
            format: Format::Deb,
            executable: "/usr/bin/herdr-gpui".into(),
        },
        offer: release::Offer {
            asset: manifest.assets[0].clone(),
            manifest,
            signature: key.sign(&manifest_bytes).to_bytes().to_vec(),
            manifest_bytes,
            notes: String::new(),
        },
    };
    write_request(&prepared)?;
    let path = prepared.stage.path();
    let (decoded, verified) = request(path, &public, "20261010.1")?;
    assert_eq!(decoded.format, Format::Deb);
    assert_eq!(verified, prepared.offer.manifest);
    assert!(matches!(
        request(path, &public, "20261010.2"),
        Err(Error::StaleRequest)
    ));
    assert!(matches!(
        request(path, &public, "dev"),
        Err(Error::StaleRequest)
    ));
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    assert!(matches!(
        request(path, &public, "20261010.1"),
        Err(Error::StagePermissions)
    ));
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    let mut tampered = decoded;
    tampered.signature[0] ^= 1;
    fs::write(path.join("request.json"), serde_json::to_vec(&tampered)?)?;
    assert!(matches!(
        request(path, &public, "20261010.1"),
        Err(Error::Signature(_))
    ));
    Ok(())
}

#[test]
fn malformed_helper_arguments_fail_before_gui_startup() {
    assert_eq!(run_helper(&[FLAG.into()]), Some(ExitCode::FAILURE));
    assert_eq!(run_helper(&[]), None);
}

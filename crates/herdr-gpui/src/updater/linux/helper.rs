//! Own the package transaction outside the GUI's lifetime. This helper is never
//! elevated; only pkexec's fixed package-manager command obtains privileges.
use super::*;
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    process::ExitCode,
};

pub(super) const FLAG: &str = "--herdr-install-package";
const LIMIT: u64 = 128 * 1024;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    executable: PathBuf,
    format: Format,
    version: String,
    manifest: Vec<u8>,
    signature: Vec<u8>,
}

pub(super) fn write_request(prepared: &Prepared) -> Result<()> {
    let request = Request {
        executable: prepared.package.executable.clone(),
        format: prepared.package.format,
        version: prepared.offer.manifest.version.clone(),
        manifest: prepared.offer.manifest_bytes.clone(),
        signature: prepared.offer.signature.clone(),
    };
    let bytes = serde_json::to_vec(&request)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::RequestLimit);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(prepared.stage.path().join("request.json"))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn request(stage: &Path, key: &str, current: &str) -> Result<(Request, release::Manifest)> {
    let uid = install::effective_uid()?;
    let meta = fs::symlink_metadata(stage)?;
    if uid == 0
        || !meta.is_dir()
        || meta.uid() != uid
        || meta.mode() & 0o7777 != 0o700
        || fs::canonicalize(stage)? != stage
    {
        return Err(Error::StagePermissions);
    }
    let path = stage.join("request.json");
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.uid() != uid || meta.nlink() != 1 || meta.mode() & 0o7777 != 0o600 {
        return Err(Error::StagePermissions);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::RequestLimit);
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    if request.format == Format::Archive
        || !request.executable.starts_with("/usr")
        || request.executable.starts_with("/usr/local")
        || release::parse_version(current).is_none()
        || release::parse_version(&request.version) <= release::parse_version(current)
    {
        return Err(Error::StaleRequest);
    }
    let manifest =
        release::verify_manifest(&request.manifest, &request.signature, key, &request.version)?;
    if manifest.schema != 2 {
        return Err(Error::ManifestSchema);
    }
    Ok((request, manifest))
}

fn apply_request(stage: &Path, key: &str) -> Result<()> {
    let (request, manifest) = request(stage, key, crate::APP_VERSION)?;
    let target = release::target().ok_or(Error::UnsupportedTarget)?;
    let name = request.format.name(&request.version, target)?;
    let asset = manifest
        .assets
        .iter()
        .find(|asset| asset.target == target && asset.name == name)
        .ok_or(Error::MissingPlatformAsset)?
        .clone();
    let package = Package {
        format: request.format,
        executable: request.executable,
    };
    let offer = release::Offer {
        manifest,
        asset,
        manifest_bytes: request.manifest,
        signature: request.signature,
        notes: String::new(),
    };
    let result = apply(
        &package,
        &offer,
        stage,
        key,
        &AtomicBool::new(false),
        |detail| {
            // A closed GUI must never turn progress output into a panic/SIGPIPE.
            let _ = writeln!(std::io::stdout(), "{detail}");
        },
    );
    // Remove only our known files, never recursively erase an argument path.
    let _ = fs::remove_file(stage.join(&offer.asset.name));
    let _ = fs::remove_file(stage.join("request.json"));
    let _ = fs::remove_dir(stage);
    result
}

#[cfg(test)]
mod tests;

pub(in crate::updater) fn run_helper(args: &[OsString]) -> Option<ExitCode> {
    if args.first().is_none_or(|arg| arg != FLAG) {
        return None;
    }
    let result = match args {
        [_, stage] => option_env!("HERDR_UPDATE_PUBLIC_KEY")
            .ok_or(Error::LocalBuild)
            .and_then(|key| apply_request(Path::new(stage), key)),
        _ => Err(Error::MalformedRequest),
    };
    Some(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            ExitCode::FAILURE
        }
    })
}

// Build-time tools run under Cargo, not from the app: no window to avoid.
#![allow(clippy::disallowed_methods)]

mod build_identity;
// Behind a default feature, like GPUI's own `windows-manifest`, so the isolated
// fixture in `tests/build_identity` still compiles this script without it.
#[cfg(all(windows, feature = "windows-resource"))]
mod build_resource;

/// The build script's own failures, as a typed error rather than the `&str`
/// Cargo accepts here. Hand-written rather than derived: `tests/build_identity`
/// recompiles this script in an isolated offline crate, so it must keep
/// building with no dependencies of its own.
#[derive(Debug)]
enum BuildError {
    MissingManifestDir,
    InvalidPrNumber,
    PrNumberEncoding(std::env::VarError),
    MockupFileEncoding,
    InvalidMockupFile,
    #[cfg(all(windows, feature = "windows-resource"))]
    Resource(ResourceError),
}

/// Failures embedding the Windows icon and version resource.
#[cfg(all(windows, feature = "windows-resource"))]
#[derive(Debug)]
enum ResourceError {
    Compile(embed_resource::CompilationResult),
    IconPathEncoding,
    InvalidReleaseVersion,
    MissingOutDir,
    ReleaseVersionEncoding(std::env::VarError),
    Write(std::io::Error),
}

#[cfg(all(windows, feature = "windows-resource"))]
impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compile(_) => f.write_str("could not compile the Windows resource script"),
            Self::IconPathEncoding => f.write_str("the Windows icon path is not valid Unicode"),
            Self::InvalidReleaseVersion => {
                f.write_str("HERDR_RELEASE_VERSION must be YYYYMMDD.COUNTER")
            }
            Self::MissingOutDir => f.write_str("OUT_DIR is not set"),
            Self::ReleaseVersionEncoding(_) => {
                f.write_str("HERDR_RELEASE_VERSION is not valid Unicode")
            }
            Self::Write(_) => f.write_str("could not write the Windows resource script"),
        }
    }
}

#[cfg(all(windows, feature = "windows-resource"))]
impl std::error::Error for ResourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Compile(error) => Some(error),
            Self::ReleaseVersionEncoding(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::IconPathEncoding | Self::InvalidReleaseVersion | Self::MissingOutDir => None,
        }
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingManifestDir => f.write_str("CARGO_MANIFEST_DIR is not set"),
            Self::InvalidPrNumber => {
                f.write_str("HERDR_BUILD_PR_NUMBER must be empty or a positive decimal integer")
            }
            Self::PrNumberEncoding(_) => f.write_str("HERDR_BUILD_PR_NUMBER is not valid Unicode"),
            Self::MockupFileEncoding => f.write_str("HERDR_MOCKUP_FILE is not valid Unicode"),
            Self::InvalidMockupFile => {
                f.write_str("HERDR_MOCKUP_FILE must be an absolute path on one line")
            }
            #[cfg(all(windows, feature = "windows-resource"))]
            Self::Resource(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PrNumberEncoding(error) => Some(error),
            #[cfg(all(windows, feature = "windows-resource"))]
            Self::Resource(error) => error.source(),
            Self::MissingManifestDir
            | Self::InvalidPrNumber
            | Self::MockupFileEncoding
            | Self::InvalidMockupFile => None,
        }
    }
}

fn main() -> Result<(), BuildError> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_identity.rs");
    println!("cargo:rerun-if-env-changed=HERDR_BUILD_PR_NUMBER");
    let manifest = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").ok_or(BuildError::MissingManifestDir)?,
    );
    let identity = build_identity::detect(&manifest);
    for path in &identity.watched {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let pr = match std::env::var("HERDR_BUILD_PR_NUMBER") {
        Ok(value) => build_identity::validate_pr(&value)
            .ok_or(BuildError::InvalidPrNumber)?
            .to_owned(),
        Err(std::env::VarError::NotPresent) if identity.worktree && !identity.detached => {
            build_identity::lookup_pr(
                &manifest,
                &identity.branch,
                std::path::Path::new("gh"),
                std::time::Duration::from_secs(2),
            )
            .unwrap_or_default()
        }
        Err(std::env::VarError::NotPresent) => String::new(),
        Err(error) => return Err(BuildError::PrNumberEncoding(error)),
    };
    println!(
        "cargo:rustc-env=HERDR_BUILD_WORKTREE={}",
        u8::from(identity.worktree)
    );
    println!("cargo:rustc-env=HERDR_BUILD_BRANCH={}", identity.branch);
    println!("cargo:rustc-env=HERDR_BUILD_PR={pr}");
    if std::env::var_os("CARGO_FEATURE_MOCKUP").is_some() {
        mockup_scratch()?;
    }
    #[cfg(all(windows, feature = "windows-resource"))]
    if std::env::var_os("CARGO_CFG_TARGET_OS").is_some_and(|os| os == "windows") {
        windows_resource(&manifest, identity.worktree).map_err(BuildError::Resource)?;
    }
    Ok(())
}

/// Embeds the icon and VERSIONINFO in herdr-gpui.exe, so Explorer, the
/// taskbar, and Task Manager show Herdr UI rather than a blank name and the
/// default icon. Linked-worktree builds get the red icon, as on macOS.
#[cfg(all(windows, feature = "windows-resource"))]
fn windows_resource(manifest: &std::path::Path, worktree: bool) -> Result<(), ResourceError> {
    println!("cargo:rerun-if-changed=build_resource.rs");
    println!("cargo:rerun-if-env-changed=HERDR_RELEASE_VERSION");
    let name = if worktree {
        "herdr-worktree.ico"
    } else {
        "herdr.ico"
    };
    let icon = manifest.join("../../assets/icons/windows").join(name);
    println!("cargo:rerun-if-changed={}", icon.display());
    let icon = icon.to_str().ok_or(ResourceError::IconPathEncoding)?;
    let version = match std::env::var("HERDR_RELEASE_VERSION") {
        Ok(version) => Some(version),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(ResourceError::ReleaseVersionEncoding(error)),
    };
    let script = build_resource::script(icon, version.as_deref())
        .ok_or(ResourceError::InvalidReleaseVersion)?;
    let out =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or(ResourceError::MissingOutDir)?)
            .join("herdr-gpui.rc");
    std::fs::write(&out, script).map_err(ResourceError::Write)?;
    // Only the app binary: test executables keep no icon of their own.
    embed_resource::compile_for(&out, ["herdr-gpui"], embed_resource::NONE)
        .manifest_optional()
        .map_err(ResourceError::Compile)
}

/// Points the `mockup` feature at the variants file an agent wrote, which
/// lives outside the repository. The crate `include!`s it by absolute path, so
/// compiler errors name the real file and rustc tracks it for rebuilds.
/// Without one, the mode shows its built-in demo.
fn mockup_scratch() -> Result<(), BuildError> {
    println!("cargo:rerun-if-env-changed=HERDR_MOCKUP_FILE");
    println!("cargo:rustc-check-cfg=cfg(herdr_mockup_scratch)");
    let Some(file) = std::env::var_os("HERDR_MOCKUP_FILE").filter(|file| !file.is_empty()) else {
        return Ok(());
    };
    let file = file
        .into_string()
        .map_err(|_| BuildError::MockupFileEncoding)?;
    if !std::path::Path::new(&file).is_absolute() || file.contains(['\n', '\r']) {
        return Err(BuildError::InvalidMockupFile);
    }
    println!("cargo:rerun-if-changed={file}");
    println!("cargo:rustc-cfg=herdr_mockup_scratch");
    println!("cargo:rustc-env=HERDR_MOCKUP_SCRATCH={file}");
    Ok(())
}

//! The installed format chooses the signed asset, never the host distribution.
use super::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(in crate::updater) enum Format {
    Archive,
    Deb,
    Rpm,
    Arch,
}

impl Format {
    pub(in crate::updater) fn name(self, version: &str, target: &str) -> Result<String> {
        if self == Self::Archive {
            return super::asset_name(version, target);
        }
        if !matches!(
            target,
            "x86_64-unknown-linux-gnu" | "aarch64-unknown-linux-gnu"
        ) {
            return Err(Error::UnsupportedTarget);
        }
        let extension = match self {
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::Arch => "pkg.tar.zst",
            Self::Archive => unreachable!(),
        };
        Ok(format!("Herdr-{version}-{target}.{extension}"))
    }

    pub(in crate::updater) fn manifest(self) -> (&'static str, &'static str) {
        if self == Self::Archive {
            ("update-manifest.json", "update-manifest.sig")
        } else {
            ("package-manifest.json", "package-manifest.sig")
        }
    }
}

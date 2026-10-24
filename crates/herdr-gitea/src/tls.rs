//! Which certificate authorities a Gitea host is trusted through. Self-hosted
//! instances are often signed by a company or home CA that the system trusts
//! but Mozilla's bundled roots do not, so the system's CA bundle is used where
//! one exists, plus a host's own `ca_file`; the bundled roots remain only for
//! systems without a readable bundle.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use ureq::tls::{Certificate, PemItem, RootCerts, parse_pem};

/// Where Linux distributions, NixOS, and macOS keep the system bundle.
const BUNDLES: [&str; 5] = [
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
    "/etc/ssl/certs/ca-bundle.crt",
];
/// A CA bundle is a few hundred kilobytes; anything far larger is not one.
const BUNDLE_LIMIT: u64 = 16 * 1024 * 1024;

type Roots = Arc<Vec<Certificate<'static>>>;

static SYSTEM: OnceLock<Roots> = OnceLock::new();
static WITH_FILES: Mutex<Vec<(PathBuf, Roots)>> = Mutex::new(Vec::new());

/// The roots a request to a host with `ca_file` (if any) verifies against.
pub(crate) fn roots(ca_file: Option<&Path>) -> RootCerts {
    let system = SYSTEM
        .get_or_init(|| {
            let variables = ["SSL_CERT_FILE", "NIX_SSL_CERT_FILE"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from);
            let found = variables
                .chain(BUNDLES.into_iter().map(PathBuf::from))
                .find_map(|path| Some(read(&path)).filter(|certs| !certs.is_empty()));
            Arc::new(found.unwrap_or_default())
        })
        .clone();
    let roots = match ca_file {
        None => system,
        Some(path) => {
            let mut cached = WITH_FILES
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            match cached.iter().find(|(known, _)| known == path) {
                Some((_, roots)) => roots.clone(),
                None => {
                    let mut certs = (*system).clone();
                    certs.extend(read(path));
                    let roots = Arc::new(certs);
                    cached.push((path.to_owned(), roots.clone()));
                    roots
                }
            }
        }
    };
    if roots.is_empty() {
        RootCerts::WebPki
    } else {
        RootCerts::Specific(roots)
    }
}

/// Every certificate in a PEM file, none when it cannot be read.
pub(crate) fn read(path: &Path) -> Vec<Certificate<'static>> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Vec::new();
    };
    if !metadata.is_file() || metadata.len() > BUNDLE_LIMIT {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    parse_pem(&bytes)
        .filter_map(|item| match item {
            Ok(PemItem::Certificate(certificate)) => Some(certificate),
            _ => None,
        })
        .collect()
}

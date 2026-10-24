//! Files and video on a Linux clipboard. A file manager or capture tool
//! publishes `text/uri-list` (wl-copy also offers it as `text/plain`), and
//! a recorder may publish only video bytes. Either becomes the clipboard's
//! files, so paste delivers them as a drop would: a local path, the image
//! bridge, or a file copy to a remote host. Text that is nothing but
//! `file://` URIs of local media pastes the same way.

use super::TEXT_LIMIT;
use crate::{Error, Result, error::ClipboardError};
use gpui::{ClipboardEntry, ClipboardItem, ExternalPaths};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// Video bytes are read whole before they are saved; screen recordings fit.
pub(super) const VIDEO_LIMIT: usize = 512 * 1024 * 1024;

/// Video types a recorder or browser publishes, and the extension each
/// saved file gets so the receiving agent recognizes it.
const VIDEO_TYPES: [(&str, &str); 5] = [
    ("video/mp4", "mp4"),
    ("video/webm", "webm"),
    ("video/quicktime", "mov"),
    ("video/x-matroska", "mkv"),
    ("video/ogg", "ogv"),
];

/// Extensions of files whose bare `file://` text pastes as the file. Any
/// other URI in plain text stays text: only `text/uri-list` names files.
const MEDIA_EXTENSIONS: [&str; 11] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "mp4", "webm", "mov", "mkv",
];

/// Runs one clipboard reader: program, arguments, byte limit, deadline.
pub(super) type Run<'a> = dyn FnMut(&str, &[&str], usize, Instant) -> Result<Option<Vec<u8>>> + 'a;

/// The clipboard's files or video, read before its text. None when it
/// offers neither, or names no local file that exists.
pub(super) fn read(
    wayland: bool,
    x11: bool,
    run: &mut Run<'_>,
    exists: &dyn Fn(&Path) -> bool,
    spill: &mut dyn FnMut(&[u8], &str) -> Result<PathBuf>,
) -> Result<Option<ClipboardItem>> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let types = offered(wayland, x11, run, deadline)?;
    if types.iter().any(|kind| kind == "text/uri-list")
        && let Some(bytes) = fetch(wayland, x11, "text/uri-list", TEXT_LIMIT, run, deadline)?
        && let Some(paths) = uri_paths(&String::from_utf8_lossy(&bytes), exists, false)
    {
        return Ok(Some(files(paths)));
    }
    for (kind, extension) in VIDEO_TYPES {
        if types.iter().any(|offered| offered == kind)
            && let Some(bytes) = fetch(wayland, x11, kind, VIDEO_LIMIT, run, deadline)?
            && !bytes.is_empty()
        {
            return Ok(Some(files(vec![spill(&bytes, extension)?])));
        }
    }
    Ok(None)
}

/// Text that is only `file://` URIs of existing local media, as the paths.
pub(super) fn text_paths(item: ClipboardItem, exists: &dyn Fn(&Path) -> bool) -> ClipboardItem {
    let paths = match item.entries() {
        [ClipboardEntry::String(text)] => uri_paths(text.text(), exists, true),
        _ => None,
    };
    paths.map_or(item, files)
}

fn files(paths: Vec<PathBuf>) -> ClipboardItem {
    ClipboardItem {
        entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(paths.into()))],
    }
}

/// The offered MIME types, from whichever reader the session has.
fn offered(wayland: bool, x11: bool, run: &mut Run<'_>, deadline: Instant) -> Result<Vec<String>> {
    for (enabled, program, args) in [
        (wayland, "wl-paste", &["--list-types"][..]),
        (
            x11,
            "xclip",
            &["-selection", "clipboard", "-t", "TARGETS", "-o"][..],
        ),
    ] {
        if enabled && let Some(bytes) = run(program, args, TEXT_LIMIT, deadline)? {
            return Ok(String::from_utf8_lossy(&bytes)
                .lines()
                .map(|line| line.trim().to_owned())
                .filter(|line| !line.is_empty())
                .collect());
        }
    }
    Ok(Vec::new())
}

fn fetch(
    wayland: bool,
    x11: bool,
    kind: &str,
    limit: usize,
    run: &mut Run<'_>,
    deadline: Instant,
) -> Result<Option<Vec<u8>>> {
    for (enabled, program, args) in [
        (wayland, "wl-paste", &["--no-newline", "--type", kind][..]),
        (
            x11,
            "xclip",
            &["-selection", "clipboard", "-t", kind, "-o"][..],
        ),
    ] {
        if enabled && let Some(bytes) = run(program, args, limit, deadline)? {
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

/// The local paths a URI list names, or None unless every entry is a
/// `file://` URI of an existing file (and, for plain text, a media file).
/// Comments and blank lines are skipped, as RFC 2483 allows.
fn uri_paths(text: &str, exists: &dyn Fn(&Path) -> bool, media_only: bool) -> Option<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let url = url::Url::parse(line).ok()?;
        if url.scheme() != "file" || !matches!(url.host_str(), None | Some("" | "localhost")) {
            return None;
        }
        let path = url.to_file_path().ok()?;
        let media = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                MEDIA_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
            });
        if (media_only && !media) || !exists(&path) {
            return None;
        }
        paths.push(path);
        // One more than a drop takes, so an oversized copy fails its limit.
        if paths.len() > super::super::file_drop::MAX_PATHS {
            break;
        }
    }
    (!paths.is_empty()).then_some(paths)
}

/// Saves copied video under the runtime directory, named by its content, so
/// pasting the same clip twice reuses one file. The file must outlive the
/// paste for a remote copy to read it; the runtime directory ends with the
/// session.
#[cfg(target_os = "linux")]
pub(super) fn spill(bytes: &[u8], extension: &str) -> Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    spill_in(&base, bytes, extension)
}

fn spill_in(base: &Path, bytes: &[u8], extension: &str) -> Result<PathBuf> {
    use sha2::{Digest, Sha256};
    use std::{fs, os::unix::fs::DirBuilderExt};

    let error = |source| Error::Clipboard(ClipboardError::Spill(source));
    let dir = base.join("herdr-gpui").join("paste");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(error)?;
    let digest = Sha256::digest(bytes);
    let name: String = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let path = dir.join(format!("clip-{name}.{extension}"));
    if !path.exists() {
        let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(error)?;
        std::io::Write::write_all(&mut file, bytes).map_err(error)?;
        file.persist(&path).map_err(|failed| error(failed.error))?;
    }
    Ok(path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn clipboard(
        offers: &[(&str, &[u8])],
    ) -> impl FnMut(&str, &[&str], usize, Instant) -> Result<Option<Vec<u8>>> {
        let offers: HashMap<String, Vec<u8>> = offers
            .iter()
            .map(|(kind, bytes)| ((*kind).to_owned(), bytes.to_vec()))
            .collect();
        move |program, args, limit, _| {
            assert_eq!(program, "wl-paste");
            let bytes = if args == ["--list-types"] {
                let mut kinds: Vec<_> = offers.keys().cloned().collect();
                kinds.sort();
                kinds.join("\n").into_bytes()
            } else {
                match offers.get(*args.last().unwrap()) {
                    Some(bytes) => bytes.clone(),
                    None => return Ok(None),
                }
            };
            assert!(bytes.len() <= limit);
            Ok(Some(bytes))
        }
    }

    fn paths(item: Option<ClipboardItem>) -> Option<Vec<PathBuf>> {
        match item?.entries() {
            [ClipboardEntry::ExternalPaths(paths)] => Some(paths.paths().to_vec()),
            _ => None,
        }
    }

    fn read_offers(offers: &[(&str, &[u8])]) -> Option<Vec<PathBuf>> {
        let mut run = clipboard(offers);
        let mut spilled = |bytes: &[u8], extension: &str| {
            assert_eq!(bytes, b"\0\0\0\x18ftypmp42");
            Ok(PathBuf::from(format!("/run/clip.{extension}")))
        };
        paths(read(true, false, &mut run, &|_| true, &mut spilled).unwrap())
    }

    #[test]
    fn a_capture_tool_uri_list_pastes_as_its_file() {
        // wl-copy --type text/uri-list also offers the URI as text/plain.
        let uri: &[u8] = b"file:///home/wolf/Screenshots/e4a2d5ab.png";
        assert_eq!(
            read_offers(&[("text/uri-list", uri), ("text/plain", uri)]),
            Some(vec![PathBuf::from("/home/wolf/Screenshots/e4a2d5ab.png")])
        );
    }

    #[test]
    fn uri_lists_decode_skip_comments_and_keep_every_file() {
        let list: &[u8] = b"# copied\r\nfile:///tmp/a%20b.mp4\r\nfile://localhost/tmp/c.txt\r\n";
        assert_eq!(
            read_offers(&[("text/uri-list", list)]),
            Some(vec![
                PathBuf::from("/tmp/a b.mp4"),
                PathBuf::from("/tmp/c.txt")
            ])
        );
    }

    #[test]
    fn web_and_remote_uris_and_missing_files_are_not_files() {
        for list in [
            &b"https://example.com/a.png"[..],
            b"file://server/share/a.png",
            b"file:///tmp/a.png\nhttps://example.com/b.png",
        ] {
            assert_eq!(read_offers(&[("text/uri-list", list)]), None);
        }
        let mut run = clipboard(&[("text/uri-list", b"file:///gone.png")]);
        let mut spill = |_: &[u8], _: &str| -> Result<PathBuf> { unreachable!() };
        assert!(
            read(true, false, &mut run, &|_| false, &mut spill)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn video_bytes_are_saved_and_pasted_as_a_file() {
        let mp4: &[u8] = b"\0\0\0\x18ftypmp42";
        assert_eq!(
            read_offers(&[("video/mp4", mp4), ("text/plain", b"clip")]),
            Some(vec![PathBuf::from("/run/clip.mp4")])
        );
    }

    #[test]
    fn plain_text_and_images_are_left_to_the_text_and_image_readers() {
        assert_eq!(read_offers(&[("text/plain", b"hello")]), None);
        assert_eq!(read_offers(&[("image/png", b"\x89PNG")]), None);
    }

    #[test]
    fn plain_text_naming_local_media_becomes_its_files() {
        let exists = |path: &Path| path != Path::new("/gone.png");
        let item = |text: &str| text_paths(ClipboardItem::new_string(text.into()), &exists);
        assert_eq!(
            paths(Some(item("file:///home/wolf/Screenshots/shot.png\n"))),
            Some(vec![PathBuf::from("/home/wolf/Screenshots/shot.png")])
        );
        for text in [
            "file:///etc/passwd",
            "file:///gone.png",
            "see file:///a.png",
            "hello",
        ] {
            assert!(matches!(
                item(text).entries(),
                [ClipboardEntry::String(entry)] if entry.text() == text
            ));
        }
    }

    #[test]
    fn spilled_video_is_private_and_reused_by_content() {
        use std::os::unix::fs::PermissionsExt;
        let runtime = tempfile::tempdir().unwrap();
        let first = spill_in(runtime.path(), b"video", "mp4").unwrap();
        assert_eq!(spill_in(runtime.path(), b"video", "mp4").unwrap(), first);
        assert_eq!(std::fs::read(&first).unwrap(), b"video");
        assert_eq!(first.extension().unwrap(), "mp4");
        let mode = std::fs::metadata(first.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}

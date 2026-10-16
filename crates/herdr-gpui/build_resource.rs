//! The resource script for herdr-gpui.exe: the icon Explorer, the taskbar, and
//! Task Manager show, and the VERSIONINFO block they read the name from.
//! Dependency-free, so `tests/build_resource` includes it on every platform.

pub const PRODUCT: &str = "Herdr UI";

/// A release's `YYYYMMDD.COUNTER` as the four 16-bit fields of a Windows
/// version, `YYYY.MMDD.COUNTER.0`: the date does not fit in one field.
pub fn file_version(version: &str) -> Option<[u16; 4]> {
    let (date, counter) = version.split_once('.')?;
    let decimal = |digits: &str| {
        !digits.is_empty() && !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit())
    };
    if date.len() != 8 || !decimal(date) || !decimal(counter) {
        return None;
    }
    Some([
        date[..4].parse().ok()?,
        date[4..].parse().ok()?,
        counter.parse().ok()?,
        0,
    ])
}

/// The `.rc` source embedding `icon`, an absolute path, as resource 1, which
/// GPUI loads for its windows. Local builds carry no release version, as in
/// the app's About box. `None` when `version` is not a release version.
/// Win32 paths cannot hold quotes or control characters, so only
/// backslashes need escaping.
pub fn script(icon: &str, version: Option<&str>) -> Option<String> {
    let (numeric, text) = match version {
        Some(version) => (file_version(version)?, version),
        None => ([0; 4], "dev"),
    };
    let numeric = numeric.map(|part| part.to_string()).join(",");
    let icon = icon.replace('\\', "\\\\");
    // Numeric flags rather than winver.h names: the script then needs no SDK
    // include path. 0x40004 is VOS_NT_WINDOWS32 and 1 is VFT_APP.
    Some(format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "{PRODUCT}"
      VALUE "FileDescription", "{PRODUCT}"
      VALUE "FileVersion", "{text}"
      VALUE "InternalName", "herdr-gpui"
      VALUE "OriginalFilename", "herdr-gpui.exe"
      VALUE "ProductName", "{PRODUCT}"
      VALUE "ProductVersion", "{text}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    ))
}

use super::*;

#[test]
fn status_icons_follow_herdr_until_one_is_chosen() -> anyhow::Result<()> {
    assert_eq!(Config::default().status_icons, None);
    assert_eq!(Config::parse("")?.status_icons, None);
    assert_eq!(Config::parse(DEFAULT_CONFIG)?.status_icons, None);
    for icons in StatusIcons::ALL {
        let text = format!("status_icons = '{}'", icons.as_str());
        assert_eq!(Config::parse(&text)?.status_icons, Some(icons));
    }
    // Herdr's own spellings belong to `ui.status_indicators`, not here.
    for invalid in ["dots", "symbols", "Solid", ""] {
        assert!(Config::parse(&format!("status_icons = '{invalid}'")).is_err());
    }
    Ok(())
}

#[test]
fn status_icons_preference_writes_and_removes_its_key() -> anyhow::Result<()> {
    use crate::config::preferences::Preference;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("local.toml");
    fs::write(&path, "# keep\nfuture = 1\nstatus_icons = 'solid' # mine\n")?;
    Config::save_preference_path(Preference::StatusIcons(Some(StatusIcons::Rail)), &path)?;
    let text = fs::read_to_string(&path)?;
    assert!(text.contains("status_icons = \"rail\" # mine"), "{text}");
    assert_eq!(Config::parse(&text)?.status_icons, Some(StatusIcons::Rail));
    Config::save_preference_path(Preference::StatusIcons(None), &path)?;
    let text = fs::read_to_string(&path)?;
    assert!(text.contains("# keep\nfuture = 1"), "{text}");
    assert_eq!(Config::parse(&text)?.status_icons, None);
    Ok(())
}

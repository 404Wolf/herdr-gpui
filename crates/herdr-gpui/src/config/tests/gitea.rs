use super::*;

#[test]
fn gitea_hosts_are_read_and_checked() -> anyhow::Result<()> {
    let config = Config::parse(
        r#"
[[gitea.hosts]]
host = "gitea.example.com"
aliases = ["gitea-ssh"]
token_command = ["cat", "/run/secrets/gitea"]

[[gitea.hosts]]
host = "code.example.org"
url = "https://code.example.org/forge"
"#,
    )?;
    assert_eq!(config.gitea.hosts.len(), 2);
    assert_eq!(
        config.gitea.hosts[0].token_command,
        ["cat", "/run/secrets/gitea"]
    );
    assert_eq!(
        config.gitea.hosts[1].url.as_deref(),
        Some("https://code.example.org/forge")
    );
    assert!(config.unknown_keys.is_empty(), "{:?}", config.unknown_keys);
    assert!(Config::parse("").map(|config| config.gitea.hosts.is_empty())?);
    assert!(matches!(
        Config::parse("[[gitea.hosts]]\nhost = \"https://gitea.example.com\""),
        Err(Error::GiteaConfig(_))
    ));
    Ok(())
}

#[test]
fn a_gitea_token_in_the_file_is_refused() {
    // A host is given a command that prints its token, never the token.
    assert!(matches!(
        Config::parse("[[gitea.hosts]]\nhost = \"gitea.example.com\"\ntoken = \"abc\""),
        Err(Error::GiteaSecretInConfig)
    ));
}

#![allow(clippy::unwrap_used)]

use super::*;

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn the_field_reads_and_writes_name_value_pairs() {
    let parsed =
        parse_env(r#" GH_HOST=mycompany.ghe.com  NOTE="two words" PATHS="a\"b" EMPTY="" "#);
    assert_eq!(
        parsed,
        Some(env(&[
            ("EMPTY", ""),
            ("GH_HOST", "mycompany.ghe.com"),
            ("NOTE", "two words"),
            ("PATHS", "a\"b"),
        ]))
    );
    let text = env_text(&parsed.unwrap());
    assert_eq!(
        text,
        r#"EMPTY="" GH_HOST=mycompany.ghe.com NOTE="two words" PATHS="a\"b""#
    );
    assert_eq!(
        parse_env(&text).map(|env| env.len()),
        Some(4),
        "round trips"
    );
    assert_eq!(parse_env(""), Some(BTreeMap::new()));
}

#[test]
fn malformed_or_unsafe_pairs_are_refused() {
    for text in [
        "GH_HOST",
        "=value",
        "1ABC=x",
        "MY-VAR=x",
        "A=\"unclosed",
        "A=\"x\"y",
        "HERDR_ACP=/tmp/evil",
    ] {
        assert_eq!(parse_env(text), None, "{text}");
    }
    assert!(check_env(&env(&[("A", "x\0y")])).is_err());
    let many: BTreeMap<String, String> = (0..=MAX_ENV)
        .map(|i| (format!("V{i}"), String::new()))
        .collect();
    assert!(matches!(
        check_env(&many),
        Err(crate::chat::Error::EnvironmentSize)
    ));
    let large = env(&[("BIG", &"x".repeat(MAX_ENV_BYTES))]);
    assert!(matches!(
        check_env(&large),
        Err(crate::chat::Error::EnvironmentSize)
    ));
}

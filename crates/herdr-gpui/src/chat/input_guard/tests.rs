use super::*;

#[test]
fn dim_or_italic_placeholder_is_not_a_draft_but_styled_pastes_are() {
    for hint in ["\x1b[2mTry searching\x1b[0m", "\x1b[3mTry searching\x1b[0m"] {
        assert!(empty(&format!("────────────\n❯ {hint}\n────────────")));
    }
    for hint in [
        "Try searching",
        "\x1b[7mx\x1b[0m",
        "\x1b[2m[Pasted text #1]\x1b[0m",
        "\x1b[2m[Image #1]\x1b[0m",
    ] {
        assert!(!empty(&format!("────────────\n❯ {hint}\n────────────")));
    }
    assert!(!empty("────────────\n❯ \x1b[2J\n────────────"));
}

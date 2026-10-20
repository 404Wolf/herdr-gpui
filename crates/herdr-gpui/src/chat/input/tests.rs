use super::*;
use core::prelude::v1::test;

#[test]
fn multiline_ime_ranges_are_utf16_but_storage_is_utf8() {
    let mut edit = Edit::default();
    edit.replace(None, "one\n😀é", false, None);
    assert_eq!(edit.text, "one\n😀é");
    edit.replace(Some(4..6), "中", true, Some(1..1));
    assert_eq!(edit.text, "one\n中é");
    assert_eq!(edit.marked, Some(4..7));
    edit.replace(None, "中文", false, None);
    assert_eq!(edit.text, "one\n中文é");
    assert!(edit.marked.is_none());
}

#[test]
fn paste_normalizes_newlines_and_is_bounded_without_splitting_characters() {
    let mut edit = Edit::default();
    edit.replace(None, "a\r\nb\rc\u{7}", false, None);
    assert_eq!(edit.text, "a\nb\nc");
    let before = edit.text.clone();
    edit.replace(None, &"é".repeat(20_000), false, None);
    assert_eq!(edit.text, before);
}

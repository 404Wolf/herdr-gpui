#![allow(clippy::unwrap_used)]
use super::*;
use tracing::Level;

fn write(path: &Path, messages: &[&str]) {
    let mut text = String::new();
    for message in messages {
        text.push_str(&serde_json::to_string(&Record::fixture(Level::INFO, *message)).unwrap());
        text.push('\n');
    }
    std::fs::write(path, text).unwrap();
}

fn messages(records: &[Arc<Record>]) -> Vec<&str> {
    records
        .iter()
        .map(|record| record.message.as_str())
        .collect()
}

#[test]
fn the_tail_takes_the_newest_records_and_reaches_into_the_rotated_file() {
    let dir = tempfile::tempdir().unwrap();
    let current = dir.path().join("herdr-gpui.jsonl");
    let previous = previous_log_file(&current);
    assert!(tail(&previous, &current, 10).unwrap().is_empty());

    write(&previous, &["one", "two"]);
    write(&current, &["three", "four"]);
    assert_eq!(messages(&tail(&previous, &current, 1).unwrap()), ["four"]);
    assert_eq!(
        messages(&tail(&previous, &current, 3).unwrap()),
        ["two", "three", "four"]
    );
    assert_eq!(
        messages(&tail(&previous, &current, 100).unwrap()),
        ["one", "two", "three", "four"]
    );
}

#[test]
fn records_print_one_line_each_and_unreadable_lines_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let current = dir.path().join("herdr-gpui.jsonl");
    write(&current, &["first"]);
    let mut text = std::fs::read_to_string(&current).unwrap();
    text.push_str("not json\n");
    text.push_str(&serde_json::to_string(&Record::fixture(Level::WARN, "second")).unwrap());
    text.push('\n');
    std::fs::write(&current, text).unwrap();
    let records = tail(&previous_log_file(&current), &current, 10).unwrap();
    let records: Vec<&Record> = records.iter().map(Arc::as_ref).collect();
    assert_eq!(
        lines_of(&records),
        "[2025-09-26 15:03:45] INFO   first\n[2025-09-26 15:03:45] WARN   second"
    );
}

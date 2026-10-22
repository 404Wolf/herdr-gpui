use super::*;
use std::{fs, process::ExitCode};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

fn output(command: &Command) -> (ExitCode, String) {
    let mut out = Vec::new();
    let code = run(command, &mut out).unwrap();
    (code, String::from_utf8(out).unwrap())
}

const NIGHTLY: &str = r#"
herdr.caps { agents_per_run = 1, tokens_per_day = 2_000_000, wall_time = "3h" }
herdr.at("03:00", { tz = "Europe/Paris" }, function(fire)
  pull("main")
  sh("just ci")
  agent { harness = "claude", worktree = "nightly-" .. fire.date, prompt = "Review main." }
end)
"#;

#[test]
fn parses_commands() {
    assert_eq!(
        parse(args(&["check", "a.luau", "--json"])),
        Ok(Command::Check {
            file: PathBuf::from("a.luau"),
            json: true,
        })
    );
    assert_eq!(parse(args(&["steps"])), Ok(Command::Steps { json: false }));
    assert_eq!(parse(args(&["--help"])), Ok(Command::Help));
    assert_eq!(parse(args(&["-V"])), Ok(Command::Version));
}

#[test]
fn refuses_bad_usage() {
    assert_eq!(parse(args(&[])), Err(Error::NoCommand));
    assert_eq!(parse(args(&["check"])), Err(Error::MissingFile));
    assert_eq!(
        parse(args(&["check", "a.luau", "b.luau"])),
        Err(Error::Unexpected("b.luau".to_owned()))
    );
    assert_eq!(
        parse(args(&["check", "--fast"])),
        Err(Error::Unexpected("--fast".to_owned()))
    );
    assert_eq!(
        parse(args(&["run"])),
        Err(Error::UnknownCommand("run".to_owned()))
    );
    assert_eq!(
        parse(args(&["--help", "x"])),
        Err(Error::Unexpected("x".to_owned()))
    );
}

#[cfg(unix)]
#[test]
fn takes_paths_that_are_not_utf8() {
    use std::os::unix::ffi::OsStringExt;
    let path = OsString::from_vec(vec![b'a', 0xff, b'.', b'l']);
    let parsed = parse([OsString::from("check"), path.clone()]).unwrap();
    assert_eq!(
        parsed,
        Command::Check {
            file: PathBuf::from(path),
            json: false,
        }
    );
}

#[test]
fn check_prints_the_plan() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nightly-validate.luau");
    fs::write(&file, NIGHTLY).unwrap();
    let (code, text) = output(&Command::Check { file, json: false });
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(text.starts_with("nightly-validate\n"), "{text}");
    assert!(text.contains("    2. sh `just ci` (arbitrary command)\n"), "{text}");
    assert!(
        text.contains("OK: 1 trigger. Every trigger can also be started by hand."),
        "{text}"
    );
}

#[test]
fn check_reports_errors_as_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("broken.luau");
    fs::write(&file, "herdr.manual(function() end)").unwrap();
    let (code, text) = output(&Command::Check { file, json: true });
    assert_eq!(code, ExitCode::FAILURE);
    let value = serde_json::from_str::<serde_json::Value>(&text).unwrap();
    assert_eq!(value["ok"], false);
    assert!(
        value["error"].as_str().unwrap().contains("no caps"),
        "{value}"
    );
}

#[test]
fn check_reports_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let (code, text) = output(&Command::Check {
        file: dir.path().join("missing.luau"),
        json: false,
    });
    assert_eq!(code, ExitCode::FAILURE);
    assert!(text.starts_with("error: could not read"), "{text}");
}

#[test]
fn steps_lists_the_catalog() {
    let (_, text) = output(&Command::Steps { json: true });
    let value = serde_json::from_str::<serde_json::Value>(&text).unwrap();
    let names = value["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["name"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let expected = Kind::ALL
        .iter()
        .map(|kind| kind.name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(names, expected);
}

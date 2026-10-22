use super::*;

fn tight() -> Limits {
    Limits {
        interrupts: 100_000,
        memory: 4 * 1024 * 1024,
        steps: 4,
    }
}

#[test]
fn a_loop_at_the_top_level_runs_out_of_budget() {
    let error = Script::load("test", &format!("{CAPS}while true do end\n"), tight()).unwrap_err();
    assert_eq!(error.budget(), Some(Budget::Interrupts));
}

#[test]
fn a_loop_in_a_handler_runs_out_of_budget() {
    let source = format!("{CAPS}herdr.manual(function() while true do end end)\n");
    let script = Script::load("test", &source, tight()).unwrap();
    let error = script
        .run(0, &fire(Reason::Manual), |_| Outcome::default())
        .unwrap_err();
    assert_eq!(error.budget(), Some(Budget::Interrupts));
}

#[test]
fn each_run_gets_a_fresh_instruction_budget() {
    let source = format!("{CAPS}herdr.manual(function() for i = 1, 20000 do end end)\n");
    let script = Script::load("test", &source, tight()).unwrap();
    for _ in 0..10 {
        script
            .run(0, &fire(Reason::Manual), |_| Outcome::default())
            .unwrap();
    }
}

#[test]
fn memory_is_capped() {
    let source = format!(
        "{CAPS}local s = string.rep(\"x\", 16 * 1024 * 1024)\nherdr.manual(function() end)\n"
    );
    let error = Script::load("test", &source, tight()).unwrap_err();
    assert_eq!(error.budget(), Some(Budget::Memory));
}

#[test]
fn steps_per_run_are_capped() {
    let source = format!("{CAPS}herdr.manual(function() for i = 1, 10 do notify(\"n\") end end)\n");
    let script = Script::load("test", &source, tight()).unwrap();
    let mut calls = 0;
    let error = script
        .run(0, &fire(Reason::Manual), |_| {
            calls += 1;
            Outcome {
                ok: true,
                ..Outcome::default()
            }
        })
        .unwrap_err();
    assert_eq!(error.budget(), Some(Budget::Steps));
    assert_eq!(calls, 4);
}

#[test]
fn a_refusal_cannot_be_caught() {
    // pcall is gone, so a budget or a stopped step always ends the run.
    let script = load("herdr.manual(function() pcall(function() end) end)\n").unwrap();
    let error = script
        .run(0, &fire(Reason::Manual), |_| Outcome::default())
        .unwrap_err();
    assert!(
        error.to_string().contains("attempt to call a nil value"),
        "{error}"
    );
}

#[test]
fn an_oversized_file_is_refused_before_it_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.luau");
    std::fs::write(
        &path,
        vec![b'-'; usize::try_from(MAX_SCRIPT_BYTES).unwrap() + 1],
    )
    .unwrap();
    let error = Script::read(&path).unwrap_err();
    assert!(matches!(error, Error::TooLarge { .. }), "{error:?}");
}

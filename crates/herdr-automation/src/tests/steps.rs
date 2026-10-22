use super::*;

fn calls(handler: &str) -> Vec<Call> {
    let script = load(&format!("herdr.manual(function(fire)\n{handler}\nend)")).unwrap();
    record(&script, 0).0
}

fn run_refusal(handler: &str) -> Refusal {
    let script = load(&format!("herdr.manual(function(fire)\n{handler}\nend)")).unwrap();
    refusal(script.run(0, &fire(Reason::Manual), |_| Outcome {
        ok: true,
        ..Outcome::default()
    }))
}

#[test]
fn each_step_reads_its_arguments() {
    let steps = calls(
        "pull(\"main\")\n\
         run(\"setup\", { check = false, timeout = \"30m\" })\n\
         sh(\"cargo build\")\n\
         worktree { name = \"spike\", base = \"main\" }\n\
         agent { harness = \"claude\", prompt = \"line one\\nline two\", worktree = \"fix\", wait = true }\n\
         notify(\"done\")",
    );
    let expected = [
        Step::Pull {
            branch: "main".to_owned(),
        },
        Step::Run {
            name: "setup".to_owned(),
        },
        Step::Sh {
            command: "cargo build".to_owned(),
        },
        Step::Worktree {
            name: "spike".to_owned(),
            base: Some("main".to_owned()),
        },
        Step::Agent {
            harness: "claude".to_owned(),
            prompt: "line one\nline two".to_owned(),
            worktree: Some("fix".to_owned()),
            base: None,
            wait: true,
        },
        Step::Notify {
            text: "done".to_owned(),
        },
    ];
    assert_eq!(
        steps
            .iter()
            .map(|call| call.step.clone())
            .collect::<Vec<_>>(),
        expected
    );
    let run = steps.get(1).unwrap();
    assert!(!run.check);
    assert_eq!(run.timeout, Some(std::time::Duration::from_secs(1_800)));
    assert!(steps.first().unwrap().check);
}

#[test]
fn arguments_are_checked() {
    let invalid = |function, problem| Refusal::Invalid { function, problem };
    assert_eq!(
        run_refusal("agent { harness = \"claude\", prompt = \"x\", model = \"big\" }"),
        invalid("agent", Problem::Unknown("model".to_owned()))
    );
    assert_eq!(
        run_refusal("agent { harness = \"claude\" }"),
        invalid("agent", Problem::Missing("prompt"))
    );
    assert_eq!(
        run_refusal("pull(\"ma\\nin\")"),
        invalid("pull", Problem::Control("branch"))
    );
    assert_eq!(
        run_refusal("pull(\"  \")"),
        invalid("pull", Problem::Empty("branch"))
    );
    assert_eq!(
        run_refusal("sh(string.rep(\"x\", 5000))"),
        invalid(
            "sh",
            Problem::TooLong {
                field: "command",
                limit: 4_096,
            }
        )
    );
    assert!(matches!(
        run_refusal("run(\"run\", \"fast\")"),
        Refusal::Invalid {
            problem: Problem::Type {
                field: "options",
                ..
            },
            ..
        }
    ));
    assert!(matches!(
        run_refusal("pull(\"main\", { timeout = \"forever\" })"),
        Refusal::Invalid {
            problem: Problem::Duration(_),
            ..
        }
    ));
}

#[test]
fn run_takes_only_worktree_scripts() {
    assert!(matches!(
        run_refusal("run(\"validation\")"),
        Refusal::Invalid {
            problem: Problem::Range { field: "name", .. },
            ..
        }
    ));
}

#[test]
fn a_checked_failure_stops_the_run() {
    let script = load("herdr.manual(function() pull(\"main\") notify(\"never\") end)").unwrap();
    let mut seen = 0;
    let end = script
        .run(0, &fire(Reason::Manual), |_| {
            seen += 1;
            Outcome::default()
        })
        .unwrap();
    assert_eq!(end, End::Stopped { step: 0 });
    assert_eq!(seen, 1);
}

#[test]
fn an_unchecked_failure_reaches_the_script() {
    let script = load(
        "herdr.manual(function()\n\
           local v = run(\"run\", { check = false })\n\
           if not v.ok then notify(\"failed with \" .. v.exit .. \": \" .. v.log_tail) end\n\
         end)",
    )
    .unwrap();
    let mut texts = Vec::new();
    let end = script
        .run(0, &fire(Reason::Manual), |call| {
            if let Step::Notify { text } = &call.step {
                texts.push(text.clone());
                return Outcome {
                    ok: true,
                    ..Outcome::default()
                };
            }
            Outcome {
                ok: false,
                exit: Some(2),
                log_tail: "test failed".to_owned(),
                ..Outcome::default()
            }
        })
        .unwrap();
    assert_eq!(end, End::Completed);
    assert_eq!(texts, ["failed with 2: test failed"]);
}

#[test]
fn calls_read_back_in_plain_words() {
    let steps = calls(
        "sh(\"rm -rf target && cargo build --release --workspace --all-features --locked\")\n\
         agent { harness = \"codex\", prompt = \"abc\", worktree = \"w\", base = \"main\" }\n\
         run(\"run\", { check = false })",
    );
    let lines = steps.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        lines,
        [
            "sh `rm -rf target && cargo build --release --workspace --all-fea…` (arbitrary command)",
            "agent codex in worktree w off main, prompt 3 chars",
            "run worktree script run · the script handles failure",
        ]
    );
}

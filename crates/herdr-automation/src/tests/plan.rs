use super::*;

const NIGHTLY: &str = r#"
herdr.at("03:00", { tz = "Europe/Paris" }, function(fire)
  pull("main")
  local v = sh("just ci", { check = false })
  if not v.ok then
    agent {
      harness = "claude",
      worktree = "fix-validation-" .. fire.date,
      prompt = "Nightly validation failed on main:\n" .. v.log_tail .. "\nFix it.",
    }
  end
  agent { harness = "claude", worktree = "nightly-" .. fire.date, prompt = "Review main." }
end)
"#;

fn plan(body: &str) -> Plan {
    Plan::of(&load(body).unwrap()).unwrap()
}

#[test]
fn the_nightly_example_shows_every_branch() {
    let plan = plan(NIGHTLY);
    let trigger = plan.triggers.first().unwrap();
    let names = trigger
        .steps
        .iter()
        .map(|step| step.call.step.kind())
        .collect::<Vec<_>>();
    assert_eq!(names, [Kind::Pull, Kind::Sh, Kind::Agent]);
    let failures = trigger
        .steps
        .iter()
        .map(|step| &step.on_failure)
        .collect::<Vec<_>>();
    let Failure::Branch(instead) = failures.get(1).unwrap() else {
        panic!("validation should branch: {failures:?}");
    };
    assert_eq!(
        instead
            .iter()
            .map(|call| call.step.kind())
            .collect::<Vec<_>>(),
        [Kind::Agent, Kind::Agent]
    );
    let Some(Step::Agent {
        worktree, prompt, ..
    }) = instead.first().map(|call| &call.step)
    else {
        panic!("the fix agent comes first");
    };
    assert_eq!(worktree.as_deref(), Some("fix-validation-<date>"));
    assert!(prompt.contains(FAKE_LOG_TAIL));
    assert_eq!(failures.first(), Some(&&Failure::Stops));
    assert_eq!(failures.get(2), Some(&&Failure::Stops));
}

#[test]
fn the_plan_reads_as_text() {
    let text = plan(NIGHTLY).to_string();
    let expected = "\
test
  caps: 2 agents per run · 2,000,000 tokens/day · 3h per run
  daily at 03:00 (Europe/Paris)
    1. pull main
    2. sh `just ci` (arbitrary command) · the script handles failure
       └ on failure, instead:
           - agent claude in worktree fix-validation-<date>, prompt 72 chars
           - agent claude in worktree nightly-<date>, prompt 12 chars
    3. agent claude in worktree nightly-<date>, prompt 12 chars
";
    assert_eq!(text, expected);
}

#[test]
fn ignoring_a_failure_continues() {
    let plan =
        plan("herdr.manual(function() run(\"run\", { check = false }) notify(\"done\") end)");
    let steps = &plan.triggers.first().unwrap().steps;
    assert_eq!(steps.first().unwrap().on_failure, Failure::Continues);
}

#[test]
fn returning_on_failure_ends_the_run() {
    let plan = plan(
        "herdr.manual(function()\n\
           if not run(\"run\", { check = false }).ok then return end\n\
           notify(\"clean\")\n\
         end)",
    );
    let steps = &plan.triggers.first().unwrap().steps;
    assert_eq!(
        steps.first().unwrap().on_failure,
        Failure::Branch(Vec::new())
    );
    assert!(plan.to_string().contains("on failure: the run ends"));
}

#[test]
fn a_manual_trigger_plans_as_manual() {
    let plan = plan("herdr.manual({ name = \"deploy\" }, function(fire) notify(fire.reason) end)");
    let trigger = plan.triggers.first().unwrap();
    assert_eq!(
        trigger.steps.first().map(|step| step.call.step.clone()),
        Some(Step::Notify {
            text: "manual".to_owned(),
        })
    );
    assert!(plan.to_string().contains("deploy: manual only"));
}

#[test]
fn the_plan_serializes_for_tools() {
    let value = serde_json::to_value(plan(NIGHTLY)).unwrap();
    assert_eq!(value["script"], "test");
    assert_eq!(value["caps"]["wall_time_seconds"], 10_800);
    let trigger = &value["triggers"][0];
    assert_eq!(trigger["schedule"]["kind"], "at");
    assert_eq!(trigger["zone"]["name"], "Europe/Paris");
    assert_eq!(trigger["steps"][0]["step"], "pull");
    assert_eq!(trigger["steps"][0]["branch"], "main");
    assert_eq!(trigger["steps"][0]["on_failure"]["then"], "stops");
    assert_eq!(trigger["steps"][1]["on_failure"]["then"], "branch");
    assert_eq!(
        trigger["steps"][1]["on_failure"]["steps"][0]["harness"],
        "claude"
    );
}

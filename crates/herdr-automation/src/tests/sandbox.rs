use super::*;

#[test]
fn unsafe_and_nondeterministic_globals_are_gone() {
    let removed = [
        "os",
        "io",
        "debug",
        "require",
        "loadstring",
        "load",
        "dofile",
        "pcall",
        "xpcall",
        "print",
        "collectgarbage",
        "gcinfo",
        "getfenv",
        "setfenv",
        "newproxy",
        "coroutine",
        "math.random",
        "math.randomseed",
    ];
    let checks = removed
        .iter()
        .map(|name| format!("assert({name} == nil, \"{name} is still reachable\")\n"))
        .collect::<String>();
    load(&format!("{checks}herdr.manual(function() end)\n")).unwrap();
}

#[test]
fn the_safe_libraries_remain() {
    load(
        "assert(string.upper(\"a\") == \"A\")\n\
         assert(math.floor(1.5) == 1)\n\
         assert(table.concat({ \"a\", \"b\" }, \",\") == \"a,b\")\n\
         assert(utf8.len(\"é\") == 1)\n\
         herdr.manual(function() end)\n",
    )
    .unwrap();
}

#[test]
fn libraries_are_frozen() {
    let error = load("string.upper = nil\nherdr.manual(function() end)\n").unwrap_err();
    assert!(matches!(error, Error::Lua(_)), "{error:?}");
}

#[test]
fn steps_are_refused_at_the_top_level() {
    assert_eq!(
        refusal(load("pull(\"main\")\nherdr.manual(function() end)\n")),
        Refusal::StepOutsideTrigger("pull")
    );
}

#[test]
fn triggers_cannot_be_declared_while_running() {
    let script = load("herdr.manual(function() herdr.manual(function() end) end)\n").unwrap();
    let result = script.run(0, &fire(Reason::Manual), |_| Outcome::default());
    assert_eq!(refusal(result), Refusal::TriggerInsideTrigger);
}

#[test]
fn every_step_has_a_global() {
    let checks = Kind::ALL
        .iter()
        .map(|kind| format!("assert(type({}) == \"function\")\n", kind.name()))
        .collect::<String>();
    load(&format!("{checks}herdr.manual(function() end)\n")).unwrap();
}

#[test]
fn step_names_are_unique() {
    let mut names = Kind::ALL.iter().map(|kind| kind.name()).collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), Kind::ALL.len());
}

#[test]
fn the_same_script_makes_the_same_calls() {
    let script = load(
        "herdr.at(\"03:00\", function(fire)\n\
           for i = 1, 3 do notify(\"pass \" .. i .. \" on \" .. fire.date) end\n\
         end)\n",
    )
    .unwrap();
    assert_eq!(record(&script, 0), record(&script, 0));
}

#[test]
fn a_script_sees_why_it_fired() {
    let script = load("herdr.at(\"03:00\", function(fire) notify(fire.reason .. \" \" .. fire.date .. \" \" .. fire.time) end)\n")
        .unwrap();
    let mut texts = Vec::new();
    script
        .run(0, &fire(Reason::Manual), |call| {
            if let Step::Notify { text } = &call.step {
                texts.push(text.clone());
            }
            Outcome {
                ok: true,
                ..Outcome::default()
            }
        })
        .unwrap();
    assert_eq!(texts, ["manual 2026-10-22 03:00"]);
}

#[test]
fn an_unknown_trigger_is_an_error() {
    let script = load("herdr.manual(function() end)\n").unwrap();
    let error = script
        .run(3, &fire(Reason::Manual), |_| Outcome::default())
        .unwrap_err();
    assert!(matches!(error, Error::UnknownTrigger(3)));
}

#[test]
fn a_syntax_error_names_the_script() {
    let error = Script::load("nightly", "herdr.caps {", Limits::default()).unwrap_err();
    assert!(error.to_string().contains("nightly"), "{error}");
}

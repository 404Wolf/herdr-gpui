use super::*;
use crate::usage::{Host, Message, REFRESH, REJECTED_LIMIT, Usage};
use std::time::Instant;

/// One read of `host` at `now` in which `claude` is rejected and `codex`
/// answers, unless `claude` is held back and so never asked.
fn read_once(usage: &mut Usage, host: &Host, now: Instant) {
    let (claude, codex) = (provider("claude"), provider("codex"));
    begin(usage, host, now);
    if !usage.current().unwrap().held.contains(&claude) {
        usage.apply(
            Message::Reading(host.clone(), claude, Err(Error::UsageRejected)),
            now,
        );
    }
    usage.apply(
        Message::Reading(host.clone(), codex, Ok(report(codex, 5.))),
        now,
    );
    usage.apply(Message::Done(host.clone(), Ok(())), now);
}

fn again(usage: &Usage, provider: Provider) -> Option<(u32, Instant)> {
    usage.current().unwrap().rejected.get(&provider).copied()
}

#[test]
fn a_rejected_sign_in_is_asked_less_often_while_others_keep_their_pace() {
    let start = Instant::now();
    let (claude, codex) = (provider("claude"), provider("codex"));
    let local = Host::Local;
    let mut usage = Usage::default();

    read_once(&mut usage, &local, start);
    assert_eq!(again(&usage, claude), Some((1, start + REFRESH * 2)));
    assert_eq!(again(&usage, codex), None);

    // The next refresh leaves Claude out but keeps what it last said, and
    // Codex is read as usual.
    let next = start + REFRESH;
    read_once(&mut usage, &local, next);
    let entry = usage.current().unwrap();
    assert!(entry.held.contains(&claude));
    assert_eq!(entry.readings.len(), 2, "a held provider is not signed out");
    let held = entry
        .readings
        .iter()
        .find(|reading| reading.provider == claude)
        .unwrap();
    assert_eq!(held.error, Some(Error::UsageRejected.to_string()));
    assert_eq!(again(&usage, claude), Some((1, start + REFRESH * 2)));

    // Once due it is asked again, and a second rejection waits twice as long.
    let due = start + REFRESH * 2;
    read_once(&mut usage, &local, due);
    assert!(!usage.current().unwrap().held.contains(&claude));
    assert_eq!(again(&usage, claude), Some((2, due + REFRESH * 4)));
}

#[test]
fn the_wait_after_rejections_stops_growing_at_its_limit() {
    let mut now = Instant::now();
    let claude = provider("claude");
    let local = Host::Local;
    let mut usage = Usage::default();
    let mut waits = Vec::new();
    for _ in 0..40 {
        read_once(&mut usage, &local, now);
        let (_, at) = again(&usage, claude).unwrap();
        waits.push(at - now);
        now = at;
    }
    assert_eq!(waits[..3], [REFRESH * 2, REFRESH * 4, REFRESH * 8]);
    assert!(waits.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(waits.last(), Some(&REJECTED_LIMIT));
}

#[test]
fn refresh_allow_and_a_good_answer_ask_a_rejected_sign_in_again() {
    let start = Instant::now();
    let claude = provider("claude");
    let local = Host::Local;

    let mut usage = Usage::default();
    read_once(&mut usage, &local, start);
    usage.refresh(start + REFRESH);
    assert_eq!(again(&usage, claude), None, "Refresh asks at once");

    let mut usage = Usage::default();
    read_once(&mut usage, &local, start);
    usage.allow(claude, start);
    assert_eq!(again(&usage, claude), None, "Allow asks at once");

    let mut usage = Usage::default();
    read_once(&mut usage, &local, start);
    let due = start + REFRESH * 2;
    begin(&mut usage, &local, due);
    usage.apply(
        Message::Reading(local.clone(), claude, Ok(report(claude, 1.))),
        due,
    );
    usage.apply(Message::Done(local.clone(), Ok(())), due);
    assert_eq!(
        again(&usage, claude),
        None,
        "a good answer forgets the rest"
    );
}

#[test]
fn a_config_change_asks_every_rejected_sign_in_again() {
    let start = Instant::now();
    let claude = provider("claude");
    let local = Host::Local;
    let mut usage = Usage::default();
    let config = UsageConfig::default();
    usage.poll(
        Some(local.clone()),
        &config,
        &Default::default(),
        0,
        false,
        start,
    );
    read_once(&mut usage, &local, start);
    usage.poll(
        Some(local.clone()),
        &config,
        &Default::default(),
        1,
        false,
        start,
    );
    assert_eq!(again(&usage, claude), None);
}

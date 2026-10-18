use crate::container::idle::{Verdict, verdict};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const AUTO_STOP: Duration = Duration::from_secs(30 * 60);

#[test]
fn a_container_first_seen_idle_starts_its_timeout_rather_than_stopping() {
    let mut seen = HashMap::new();
    let start = Instant::now();
    assert_eq!(
        verdict(&mut seen, "c", false, start, AUTO_STOP),
        Verdict::Keep
    );
    assert_eq!(
        verdict(&mut seen, "c", false, start + AUTO_STOP / 2, AUTO_STOP),
        Verdict::Keep
    );
    assert_eq!(
        verdict(&mut seen, "c", false, start + AUTO_STOP, AUTO_STOP),
        Verdict::Stop
    );
    assert!(seen.is_empty(), "a stopped container starts over");
}

#[test]
fn an_agent_at_work_holds_the_container_off() {
    let mut seen = HashMap::new();
    let start = Instant::now();
    verdict(&mut seen, "c", false, start, AUTO_STOP);
    let later = start + AUTO_STOP - Duration::from_secs(60);
    assert_eq!(
        verdict(&mut seen, "c", true, later, AUTO_STOP),
        Verdict::Keep
    );
    assert_eq!(
        verdict(&mut seen, "c", false, start + AUTO_STOP, AUTO_STOP),
        Verdict::Keep,
        "the timeout restarts from the last work"
    );
    assert_eq!(
        verdict(&mut seen, "c", false, later + AUTO_STOP, AUTO_STOP),
        Verdict::Stop
    );
}

#[test]
fn auto_stop_is_never_or_at_least_five_minutes() {
    use crate::container::settings::auto_stop;
    assert_eq!(auto_stop("never"), Some(None));
    assert_eq!(auto_stop("30m"), Some(Some(Duration::from_secs(1800))));
    assert_eq!(auto_stop("2h"), Some(Some(Duration::from_secs(7200))));
    for bad in ["4m", "0h", "30", "m", "30s", "-5m", ""] {
        assert_eq!(auto_stop(bad), None, "{bad}");
    }
}

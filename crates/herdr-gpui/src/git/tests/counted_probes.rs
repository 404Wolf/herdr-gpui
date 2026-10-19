use super::*;

#[test]
fn counting_listed_checkouts_reprobes_dirty_answers_and_keeps_the_backoff() {
    let mut peer = Peer::new();
    let now = Instant::now();
    let listed = [input("feature"), input("other")];
    peer.git.track(Some(input("feature")), true, now);
    peer.git.track_listed(listed.clone(), true, now);
    peer.git.poll(now);
    let (requested, _) = peer.request();
    peer.complete(requested, Completion::Status(Ok(status(5, 1, 0))), now);
    // The focused checkout's counts come from its own status.
    assert_eq!(
        peer.git.counts("/repo/.git", "feature"),
        Some(status(5, 1, 0))
    );
    peer.git.poll(now);
    let (requested, job) = peer.request();
    assert!(
        matches!(job, Job::Dirty),
        "dirt only, until counts are wanted"
    );
    peer.complete(requested, Completion::Dirty(Ok(true)), now);
    assert_eq!(peer.git.counts("/repo/.git", "other"), None);

    // Turning counting on asks again at the next scan, not after the refresh.
    peer.git.set_counted(true);
    let next = now + SCAN_INTERVAL;
    peer.git.track_listed(listed.clone(), true, next);
    peer.git.poll(next);
    let (requested, job) = peer.request();
    assert_eq!(requested, input("other"));
    assert!(matches!(job, Job::Counts));
    peer.complete(requested, Completion::Counts(Ok(status(42, 7, 2))), next);
    assert_eq!(
        peer.git.counts("/repo/.git", "other"),
        Some(status(42, 7, 2))
    );
    assert_eq!(peer.git.dirty("/repo/.git", "other"), Some(true));

    // A counted answer waits out the usual refresh.
    let early = next + PROBE_REFRESH - Duration::from_secs(1);
    peer.git.track_listed(listed.clone(), true, early);
    peer.git.poll(early);
    assert!(peer.incoming.try_recv().is_err());

    // A failure is unknown, and backs off rather than retrying every scan.
    let due = next + PROBE_REFRESH;
    peer.git.track_listed(listed.clone(), true, due);
    peer.git.poll(due);
    let (requested, job) = peer.request();
    assert!(matches!(job, Job::Counts));
    peer.complete(
        requested,
        Completion::Counts(Err(Error::GitNoCheckout)),
        due,
    );
    assert_eq!(peer.git.counts("/repo/.git", "other"), None);
    assert_eq!(peer.git.dirty("/repo/.git", "other"), None);
    let later = due + SCAN_INTERVAL;
    peer.git.track_listed(listed.clone(), true, later);
    peer.git.poll(later);
    assert!(peer.incoming.try_recv().is_err(), "a failure waits longer");

    // Counted answers still mark dirt once counting is off again.
    peer.git.set_counted(false);
    peer.git.seed_counts(input("other"), status(0, 0, 1), later);
    assert_eq!(peer.git.dirty("/repo/.git", "other"), Some(true));
    assert_eq!(
        peer.git.counts("/repo/.git", "other"),
        Some(status(0, 0, 1))
    );
}

#[test]
fn a_counted_probe_reads_lines_against_head() {
    let directory = tempfile::tempdir().unwrap();
    let checkout = directory.path().to_str().unwrap();
    let command = |args: &[&str]| {
        let result = Command::new("git")
            .args(["-C", checkout, "-c", "user.name=Test"])
            .args([
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    command(&["init", "-b", "test"]);
    std::fs::write(directory.path().join("tracked"), "one\ntwo\nthree\n").unwrap();
    command(&["add", "tracked"]);
    command(&["commit", "--no-verify", "-m", "initial"]);
    std::fs::write(directory.path().join("tracked"), "one\n2\n3\nfour\n").unwrap();
    std::fs::write(directory.path().join("new"), "new\n").unwrap();
    let input = Input {
        repo_key: Some(directory.path().join(".git").to_str().unwrap().to_owned()),
        branch: "test".into(),
        checkout: Some(checkout.into()),
    };
    let Completion::Counts(result) = execute(&input, Job::Counts, &|| false) else {
        panic!("a counted probe answers with counts");
    };
    assert_eq!(result.unwrap(), status(3, 2, 1));
}

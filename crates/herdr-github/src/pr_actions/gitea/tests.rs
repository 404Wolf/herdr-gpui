#![allow(clippy::unwrap_used)]

use super::{super::Action, assemble, commented, complete, style, target};
use crate::pull_request::{MergeMethod, PullRequest};
use herdr_gitea::{
    Comment as GiteaComment, Config, HostConfig, Review as GiteaReview, ReviewComment, User,
    testing::{Server, ok},
};
use serde_json::json;

fn user(login: &str) -> Option<User> {
    Some(User {
        login: login.into(),
    })
}

#[test]
fn the_conversation_lists_comments_verdicts_and_one_entry_per_file() {
    let comments = [GiteaComment {
        body: "Looks\ngood".into(),
        user: user("a"),
        created_at: "2026-01-01T00:00:03Z".into(),
        ..GiteaComment::default()
    }];
    let reviews = [
        GiteaReview {
            state: "APPROVED".into(),
            user: user("b"),
            submitted_at: "2026-01-01T00:00:02Z".into(),
            ..GiteaReview::default()
        },
        // Only wraps inline comments, so it is not listed itself.
        GiteaReview {
            state: "COMMENT".into(),
            user: user("c"),
            submitted_at: "2026-01-01T00:00:01Z".into(),
            ..GiteaReview::default()
        },
        GiteaReview {
            state: "PENDING".into(),
            body: "draft".into(),
            ..GiteaReview::default()
        },
        GiteaReview {
            state: "REQUEST_CHANGES".into(),
            dismissed: true,
            body: "never mind".into(),
            user: user("d"),
            submitted_at: "2026-01-01T00:00:04Z".into(),
            ..GiteaReview::default()
        },
    ];
    let threads = [
        ReviewComment {
            path: "src/lib.rs".into(),
            body: "why?".into(),
            user: user("c"),
            created_at: "2026-01-01T00:00:00Z".into(),
            ..ReviewComment::default()
        },
        ReviewComment {
            path: "src/lib.rs".into(),
            body: "because".into(),
            user: user("a"),
            created_at: "2026-01-01T00:00:05Z".into(),
            resolver: user("a"),
            ..ReviewComment::default()
        },
    ];
    let entries = assemble(&comments, &reviews, &threads);
    let labels: Vec<(String, String)> = entries
        .iter()
        .map(|entry| (entry.author.clone(), entry.kind.label()))
        .collect();
    assert_eq!(
        labels,
        [
            ("c".to_owned(), "on src/lib.rs, 1 reply".to_owned()),
            ("b".to_owned(), "approved".to_owned()),
            ("a".to_owned(), "commented".to_owned()),
            ("d".to_owned(), "review dismissed".to_owned()),
        ]
    );
    assert_eq!(entries[2].body, "Looks good");
    assert_eq!(entries[2].source, "Looks\ngood");
}

#[test]
fn merge_methods_name_gitea_styles() {
    assert_eq!(style(MergeMethod::Merge), "merge");
    assert_eq!(style(MergeMethod::Squash), "squash");
    assert_eq!(style(MergeMethod::Rebase), "rebase");
}

/// The configured hosts are process-wide, so tests that set them take turns.
static REGISTRY: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn configure(base: &str) -> std::sync::MutexGuard<'static, ()> {
    let guard = REGISTRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    herdr_gitea::configure(&Config {
        hosts: vec![HostConfig {
            host: "127.0.0.1".into(),
            url: Some(base.into()),
            ..HostConfig::default()
        }],
    });
    guard
}

fn pull_request(base: &str, number: u64) -> PullRequest {
    let mut pr = crate::pull_request::fixture().unwrap();
    pr.number = number;
    pr.url = format!("{base}/o/r/pulls/{number}");
    pr.id = format!("gitea:{number}");
    pr
}

#[test]
fn a_merge_and_a_comment_are_each_sent_once_as_gitea_asks() {
    let server = Server::start_with(|base| {
        vec![
            ok("POST", "/api/v1/repos/o/r/pulls/41/merge", json!(null)),
            ok(
                "POST",
                "/api/v1/repos/o/r/issues/41/comments",
                json!({"html_url": format!("{base}/o/r/pulls/41#issuecomment-9")}),
            ),
        ]
    })
    .unwrap();
    let _registry = configure(&server.base);
    let pr = pull_request(&server.base, 41);
    let target = target(&pr, true).unwrap().unwrap();
    assert_eq!((target.owner.as_str(), target.repo.as_str()), ("o", "r"));
    let merged = complete(
        &target,
        super::Job::Run(Action::Merge(MergeMethod::Squash)),
        || false,
    );
    let super::Completion::Action(Ok(outcome)) = merged else {
        panic!("merge failed");
    };
    assert_eq!(outcome.message, "Merged pull request #41");
    let commented = complete(
        &target,
        super::Job::Run(Action::Comment("hi".into())),
        || false,
    );
    let super::Completion::Action(Ok(outcome)) = commented else {
        panic!("comment failed");
    };
    assert_eq!(
        outcome.url,
        Some(format!("{}/o/r/pulls/41#issuecomment-9", server.base))
    );
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    let merge: serde_json::Value = serde_json::from_str(&requests[0].3).unwrap();
    assert_eq!(merge["Do"], "squash");
    assert_eq!(merge["head_commit_id"], pr.head_ref_oid);
    let comment: serde_json::Value = serde_json::from_str(&requests[1].3).unwrap();
    assert_eq!(comment["body"], "hi");
}

#[test]
fn a_github_pull_request_is_not_a_gitea_target() {
    let pr = crate::pull_request::fixture().unwrap();
    assert!(target(&pr, true).is_none());
    // A Gitea ID on an address no configured host serves is refused.
    let _registry = configure("https://gitea.example.com");
    let mut forged = pr;
    forged.id = format!("gitea:{}", forged.number);
    assert!(matches!(target(&forged, true), Some(Err(_))));
}

#[test]
fn a_comment_link_is_kept_only_on_this_pull_request() {
    let _registry = configure("https://gitea.example.com");
    let pr = pull_request("https://gitea.example.com", 5);
    let target = target(&pr, true).unwrap().unwrap();
    let outcome = commented(
        &json!({"html_url": "https://gitea.example.com/o/r/pulls/5#issuecomment-12"}),
        &target,
    );
    assert!(outcome.url.is_some());
    let outcome = commented(
        &json!({"html_url": "https://evil.example/o/r/pulls/5#issuecomment-12"}),
        &target,
    );
    assert!(outcome.url.is_none());
}

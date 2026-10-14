use super::*;
use crate::pull_request::{
    batch::query,
    fetch::{Ask, Head, Target},
};

fn target(owner: &str, ask: Ask) -> (Input, Target) {
    (
        Input {
            checkout: None,
            repo_key: Some("/repo/.git".into()),
            branch: "local".into(),
        },
        Target {
            owner: owner.into(),
            repo: "project".into(),
            ask,
        },
    )
}

#[test]
fn one_query_asks_each_checkout_under_its_own_alias_and_variables() {
    let targets = [
        target(
            "example",
            Ask::Branch {
                branch: "feature".into(),
                head: Some(Head {
                    owner: "example".into(),
                    repo: "project".into(),
                    branch: "feature".into(),
                }),
            },
        ),
        target(
            "other\") { evil }",
            Ask::Branch {
                branch: "topic".into(),
                head: None,
            },
        ),
        target("example", Ask::Number(42)),
    ];
    let (text, variables, aliases) = query(&targets);
    assert_eq!(
        aliases,
        [
            ("p0".into(), "example/project".into()),
            ("p1".into(), "other\") { evil }/project".into()),
            ("p2".into(), "example/project".into()),
        ]
    );
    assert!(text.contains("p0: repository(owner: $o0, name: $r0)"));
    assert!(text.contains("pullRequests(first: $l1, headRefName: $b1"));
    assert!(text.contains("p2: repository(owner: $o2, name: $r2) { mergeCommitAllowed"));
    assert!(text.contains("pullRequest(number: $n2)"));
    assert!(text.contains("fragment PullRequestFields on PullRequest"));
    // Names travel only as variables, never in the query text.
    assert!(!text.contains("evil") && !text.contains("feature"));
    assert_eq!(variables["o1"], "other\") { evil }");
    assert_eq!(variables["b0"], "feature");
    assert_eq!(variables["l0"], 100);
    assert_eq!(variables["l1"], 2);
    assert_eq!(variables["n2"], 42);
}

#[test]
fn each_alias_parses_as_a_single_lookup() {
    let (_, target) = target(
        "example",
        Ask::Branch {
            branch: "feature".into(),
            head: None,
        },
    );
    let mut nodes = response();
    nodes[0]["commits"] = serde_json::json!({"nodes": []});
    let answer = serde_json::json!({"data": {"repository": {
        "squashMergeAllowed": true,
        "pullRequests": {"pageInfo": {"hasNextPage": false}, "nodes": nodes},
    }}});
    let pr = crate::pull_request::batch::parse(answer, &target)
        .unwrap()
        .unwrap();
    assert_eq!(pr.number, 8);
}

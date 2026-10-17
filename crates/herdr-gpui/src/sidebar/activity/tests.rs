#![allow(clippy::unwrap_used)]

use super::{ActivityPanel, BAR_WIDTH};
use crate::{
    HerdrWindow,
    browser::TabId,
    orchestrator::{Item, ItemKey, Owner, Provider, Run, RunState, SourceKey},
};

mod layout;
mod orchestrator_rows;
mod rows;

fn item(native_id: &str, title: &str) -> Item {
    Item {
        key: ItemKey {
            source: SourceKey {
                provider: Provider::Github,
                host: "github.com".into(),
                repository: "penso/herdr-gpui".into(),
            },
            native_id: native_id.into(),
        },
        identifier: format!("#{native_id}"),
        title: title.into(),
        description: None,
        state: "open".into(),
        url: None,
        author: None,
        labels: vec!["enhancement".into()],
        parent_id: None,
        blocked_by: Vec::new(),
        priority: None,
        created_at: None,
        updated_at: None,
        pull_request: None,
        activity: None,
    }
}

fn run(id: &str, item: &Item, state: RunState) -> Run {
    Run {
        id: id.into(),
        item_key: item.key.canonical(),
        workspace: None,
        agent: "claude".into(),
        model: None,
        state,
        message: None,
        session_id: None,
        started_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        owner: Owner::HerdrGpui,
    }
}

fn tab() -> super::rows::Host {
    super::rows::Host::Tab(TabId::test(7))
}

fn pull_request(number: u64) -> crate::orchestrator::PullRequest {
    crate::orchestrator::PullRequest {
        number,
        additions: None,
        deletions: None,
        base_ref: "main".into(),
        head_ref: format!("b-{number}"),
        base_sha: String::new(),
        head_sha: String::new(),
        head_repository: None,
    }
}

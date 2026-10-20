//! Synthetic conversation shared by native visual QA and headless UI tests.
use herdr_client::chat::{Conversation, Part, Role, Turn};

pub(super) fn conversation() -> Conversation {
    Conversation { turns: vec![
        Turn { id: "request".into(), role:Role::User, parts:vec![Part::Text("Fix the crash when a search has no results. Add a regression test.".into())] },
        Turn { id: "response".into(), role:Role::Assistant, parts:vec![
            Part::Text("The empty list is indexed before it is checked. I’ll add an early return.".into()),
            Part::Tool { id:"read".into(), name:"Read".into(), input:"src/search.rs".into(), output:Some("let first = results[0];".into()), failed:false },
            Part::Tool { id:"test".into(), name:"Bash".into(), input:"cargo test search".into(), output:Some("12 tests passed; 0 failed".into()), failed:false },
            Part::Text("### Empty searches are handled.\n\nEmpty input now returns `[]` before indexing.\n\n- Added the regression case\n- **12 tests passed**".into()),
        ] },
    ], model: Some("Claude (fixture)".into()), ..Default::default() }
}

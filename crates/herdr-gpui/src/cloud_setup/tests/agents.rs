use super::*;

#[test]
fn every_agent_has_a_distinct_key_that_names_it() {
    for agent in Agent::ALL {
        assert_eq!(Agent::from_key(agent.key()), Some(*agent));
        let same = Agent::ALL.iter().filter(|other| other.key() == agent.key());
        assert_eq!(same.count(), 1, "{} is not unique", agent.key());
    }
    assert_eq!(Agent::from_key("cursor"), None);
}

#[test]
fn every_install_skips_a_tool_already_there() {
    for agent in Agent::ALL {
        let script = agent.script();
        assert!(
            script.contains(&format!("command -v {} ", agent.key())),
            "{} reinstalls itself",
            agent.key()
        );
        assert!(script.contains(agent.label()));
        assert!(!script.contains('\0'));
    }
}

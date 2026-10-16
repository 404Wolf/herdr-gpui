//! The category list runs group by group, each group's pages together and the
//! groups in their own order, so a new section has one obvious place to go:
//! the end of its group.
use super::*;

#[test]
fn sections_run_group_by_group_with_general_leading_behaviour() {
    let groups: Vec<_> = Section::ALL.iter().map(|section| section.group()).collect();
    let mut grouped = groups.clone();
    grouped.sort_by_key(|group| Group::ALL.iter().position(|candidate| candidate == group));
    assert_eq!(groups, grouped);
    // General stays first in Behaviour, as the catch-all people look in when
    // they do not know where a setting lives.
    let behaviour = Section::ALL
        .iter()
        .find(|section| section.group() == Group::Behaviour);
    assert_eq!(behaviour, Some(&Section::General));
}

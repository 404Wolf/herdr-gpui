//! The category list keeps General on top and the rest alphabetical, so a
//! new section has one obvious place to go.
use super::*;

#[test]
fn general_leads_and_remaining_sections_are_alphabetical() {
    let (first, rest) = Section::ALL.split_first().unwrap();
    assert_eq!(*first, Section::General);
    let labels: Vec<_> = rest.iter().map(|section| section.label()).collect();
    let mut sorted = labels.clone();
    sorted.sort_by_key(|label| label.to_lowercase());
    assert_eq!(labels, sorted);
}

use super::*;

#[test]
fn a_wanted_checkpoint_is_selected_once_listed_and_never_confirmed() {
    let mut checkpoints = Checkpoints::default();
    checkpoints.open(feature(), "repo".into(), "w4".into());
    checkpoints.want("1".into());
    answer(
        &mut checkpoints,
        Request::List,
        Answer::Listed(Ok(vec![
            checkpoint("2", "feature"),
            checkpoint("1", "feature"),
        ])),
    );
    let view = checkpoints.view.as_ref().unwrap();
    assert_eq!(view.selected, Some(1));
    assert_eq!(view.confirming, None, "restoring still takes confirming");
    assert_eq!(view.wanted, None, "applied once");
    // A later list keeps the user's own selection.
    checkpoints.view.as_mut().unwrap().selected = Some(0);
    answer(
        &mut checkpoints,
        Request::List,
        Answer::Listed(Ok(vec![
            checkpoint("2", "feature"),
            checkpoint("1", "feature"),
        ])),
    );
    assert_eq!(checkpoints.view.as_ref().unwrap().selected, Some(0));
    // A checkpoint that is gone selects nothing new.
    checkpoints.want("9".into());
    answer(
        &mut checkpoints,
        Request::List,
        Answer::Listed(Ok(vec![checkpoint("2", "feature")])),
    );
    assert_eq!(checkpoints.view.as_ref().unwrap().selected, Some(0));
}

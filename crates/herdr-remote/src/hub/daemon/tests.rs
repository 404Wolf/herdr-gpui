use super::*;

#[test]
fn a_request_given_up_on_frees_its_slot() {
    let pending = Mutex::new(HashMap::new());
    for n in 0..MAX_PENDING * 2 {
        let (sender, _receiver) = oneshot::channel();
        let id = format!("r{n}");
        lock(&pending).insert(id.clone(), sender);
        // A timeout or a cancelled caller drops the slot unanswered.
        drop(Slot {
            pending: &pending,
            id,
        });
    }
    assert!(lock(&pending).is_empty());
}

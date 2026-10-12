use super::*;

#[test]
fn a_huge_wait_is_clamped_instead_of_overflowing() {
    let now = Instant::now();
    assert_eq!(
        deadline(Duration::MAX, now),
        now + Duration::from_secs(MAX_WAIT_SECONDS)
    );
    assert_eq!(
        deadline(Duration::from_secs(5), now),
        now + Duration::from_secs(5)
    );
}

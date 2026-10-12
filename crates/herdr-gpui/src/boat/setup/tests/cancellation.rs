use super::*;

#[test]
fn already_cancelled_jobs_exit_before_preparing_ssh_or_credentials() {
    let request = AddRequest {
        settings: crate::boat::tests::settings(),
        name: "herdr-box".into(),
        install: true,
    };
    let result = add_device(request, &|| true, &|_| {
        panic!("cancelled jobs must not report creation")
    });
    assert!(matches!(
        result,
        Err(crate::Error::Boat(crate::boat::Error::Cancelled))
    ));
}

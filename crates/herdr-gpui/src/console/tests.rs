use super::*;

#[test]
fn only_a_console_created_for_this_process_is_owned() {
    assert!(owns_console(1));
    // Started from a shell that still shares the console.
    assert!(!owns_console(2));
    // GetConsoleProcessList failed, or there is no console.
    assert!(!owns_console(0));
}

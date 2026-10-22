use super::*;

#[test]
fn raw_html_in_agent_text_is_shown_as_text() {
    let html = markdown("hello <script>alert(1)</script> <b>x</b>").into_string();
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<b>"));
}

#[test]
fn only_safe_link_schemes_keep_their_target() {
    let html = markdown("[a](javascript:alert(1)) [b](https://example.com) [c](data:text/html,x)")
        .into_string();
    assert!(!html.contains("javascript:"));
    assert!(!html.contains("data:text"));
    assert!(html.contains(r#"href="https://example.com""#));
    assert!(html.contains("noopener"));
}

#[test]
fn ids_are_percent_encoded_for_urls() {
    assert_eq!(urlencode("w1:p 2/x"), "w1%3Ap%202%2Fx");
    assert_eq!(urlencode("plain-id_1.2~"), "plain-id_1.2~");
}

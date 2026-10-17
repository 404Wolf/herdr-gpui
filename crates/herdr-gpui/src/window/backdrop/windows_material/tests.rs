use super::*;

#[test]
fn blur_uses_dwm_only_where_it_has_system_backdrops() {
    assert_eq!(route(Appearance::Blurred, true), Route::Dwm);
    assert_eq!(route(Appearance::Blurred, false), Route::Accent);
    assert_eq!(route(Appearance::MicaBackdrop, true), Route::Dwm);
    assert_eq!(route(Appearance::Transparent, true), Route::Accent);
    assert_eq!(route(Appearance::Opaque, true), Route::None);
}

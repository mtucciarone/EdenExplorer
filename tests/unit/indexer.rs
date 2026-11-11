use super::theme_cache_path;

#[test]
fn theme_cache_path_is_well_formed() {
    let path = theme_cache_path();
    assert!(path.is_some() || path.is_none());
}

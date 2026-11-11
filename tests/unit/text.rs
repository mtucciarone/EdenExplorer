use super::expand_environment_variables;

#[test]
fn expands_environment_variables() {
    if let Ok(appdata) = std::env::var("APPDATA") {
        assert_eq!(expand_environment_variables("%appdata%"), appdata);
    }
    if let Ok(windir) = std::env::var("WINDIR") {
        assert_eq!(expand_environment_variables("%WiNdIr%"), windir);
    }
    assert_eq!(
        expand_environment_variables("%nonexistent%"),
        "%nonexistent%"
    );
    assert_eq!(
        expand_environment_variables("C:\\Windows\\System32"),
        "C:\\Windows\\System32"
    );
    assert_eq!(expand_environment_variables(""), "");
}

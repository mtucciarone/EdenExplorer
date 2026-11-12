use super::*;

#[test]
fn structs_match_the_nvapi_layout() {
    // Sizes from nvapi.h; the driver rejects a struct whose version (which
    // carries the size) is off by even one byte.
    assert_eq!(size_of::<Setting>(), 12320);
    assert_eq!(size_of::<Profile>(), 4116);
    assert_eq!(size_of::<Application>(), 12296);
    assert_eq!(struct_version::<Setting>(1), 0x0001_3020);
    assert_eq!(std::mem::offset_of!(Setting, id), 4100);
    assert_eq!(std::mem::offset_of!(Setting, current), 8220);
}

/// Talks to the real driver, so it only runs when asked for:
/// `cargo test driver_round_trip -- --ignored --nocapture`.
#[test]
#[ignore]
fn driver_round_trip() {
    let started = std::time::Instant::now();
    let outcome = disable_gsync_for_this_exe();
    println!("{outcome:?} in {:?}", started.elapsed());
}

#[test]
fn a_new_driver_or_a_renamed_exe_changes_the_stamp() {
    let stamp = stamp_text("edenexplorer.exe", 100, 5);
    assert_ne!(stamp, stamp_text("edenexplorer.exe", 101, 5));
    assert_ne!(stamp, stamp_text("edenexplorer.exe", 100, 6));
    assert_ne!(stamp, stamp_text("eden.exe", 100, 5));
}

#[test]
fn marker_is_bound_to_the_driver_exe_and_profile() {
    let stamp = stamp_text("edenexplorer.exe", 100, 5);
    let profile = "EdenExplorerNVIDIA-test-pc";
    let marker = marker_text(&stamp, profile);
    assert!(marker_matches(&marker, &stamp, profile));
    assert!(!marker_matches(
        &marker,
        &stamp_text("eden.exe", 100, 5),
        profile
    ));
    assert!(!marker_matches(
        &marker,
        &stamp,
        "EdenExplorerNVIDIA-other-pc"
    ));
    assert!(!marker_matches("v1\nanything", &stamp, profile));
}

#[test]
fn profile_name_has_a_vendor_and_machine_specific_namespace() {
    let profile = profile_name_for("office-pc_01");
    assert!(profile.starts_with(PROFILE_PREFIX));
    assert!(
        profile[PROFILE_PREFIX.len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    );
    assert!(profile.len() <= PROFILE_PREFIX.len() + 64);
}

#[test]
fn profile_name_sanitizes_and_bounds_machine_names() {
    let profile = profile_name_for("lab.pc/01");
    assert_eq!(profile, "EdenExplorerNVIDIA-lab_pc_01");
    assert_eq!(
        profile_name_for(&"x".repeat(100)).len(),
        PROFILE_PREFIX.len() + 64
    );
    assert_eq!(profile_name_for("..."), "EdenExplorerNVIDIA-___");
}

#[test]
fn an_unknown_exe_gets_a_profile() {
    assert_eq!(plan(Existing::NoProfile), Plan::CreateProfileAndAddSetting);
}

#[test]
fn an_existing_profile_only_gains_the_setting() {
    assert_eq!(plan(Existing::ProfileWithoutSetting), Plan::AddSetting);
}

#[test]
fn a_gsync_value_someone_already_chose_is_kept() {
    assert_eq!(plan(Existing::ProfileWithSetting), Plan::Nothing);
}

#[test]
fn an_existing_force_off_setting_is_confirmed_without_a_write() {
    assert_eq!(plan(Existing::ProfileWithGsyncOff), Plan::AlreadyDisabled);
}

#[test]
fn every_profile_state_has_an_explicit_plan() {
    assert_eq!(plan(Existing::NoProfile), Plan::CreateProfileAndAddSetting);
    assert_eq!(plan(Existing::ProfileWithoutSetting), Plan::AddSetting);
    assert_eq!(plan(Existing::ProfileWithSetting), Plan::Nothing);
    assert_eq!(plan(Existing::ProfileWithGsyncOff), Plan::AlreadyDisabled);
}

#[test]
fn unicode_strings_are_nul_terminated() {
    let text = unicode_string("eden.exe").unwrap();
    assert_eq!(String::from_utf16_lossy(&text[..8]), "eden.exe");
    assert_eq!(text[8], 0);
    assert!(unicode_string(&"x".repeat(2047)).is_some());
    assert!(unicode_string(&"x".repeat(2048)).is_none());
}

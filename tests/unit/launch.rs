use super::*;
use std::path::PathBuf;

#[test]
fn parses_multiple_paths_and_new_window() {
    let options = parse_args([
        "EdenExplorer.exe",
        "--new-window",
        "C:\\Projects",
        "D:\\Archive",
    ])
    .unwrap();
    assert!(options.new_window);
    assert_eq!(
        options.paths,
        vec![PathBuf::from("C:\\Projects"), PathBuf::from("D:\\Archive")]
    );
}

#[test]
fn allows_dash_prefixed_paths_after_double_dash() {
    let options = parse_args(["EdenExplorer.exe", "--", "-folder"]).unwrap();
    assert_eq!(options.paths, vec![PathBuf::from("-folder")]);
}

#[test]
fn rejects_unknown_options() {
    assert!(matches!(
        parse_args(["EdenExplorer.exe", "--unknown"]).unwrap_err(),
        LaunchError::UnknownOption(_)
    ));
}

#[test]
fn startup_i18n_loads() {
    assert_ne!(LaunchI18n::new().tr("launch-window-not-ready"), "");
}

#[test]
fn normalizes_my_pc_aliases() {
    for alias in [
        "::MY_PC::",
        "file:\\",
        "shell:MyComputerFolder",
        "SHELL:MYCOMPUTERFOLDER",
    ] {
        assert_eq!(
            parse_args(["EdenExplorer.exe", alias]).unwrap().paths,
            vec![PathBuf::from(MY_PC_PATH)]
        );
    }
}

#[test]
fn normalizes_recycle_bin_aliases() {
    for alias in [
        "::RECYCLE_BIN::",
        "shell:RecycleBinFolder",
        "SHELL:RECYCLEBINFOLDER",
    ] {
        assert_eq!(
            parse_args(["EdenExplorer.exe", alias]).unwrap().paths,
            vec![PathBuf::from(MY_RECYCLE_BIN_PATH)]
        );
    }
}

#[test]
fn preserves_normal_directories() {
    let options = parse_args(["EdenExplorer.exe", "C:\\Projects", "D:\\Archive"]).unwrap();
    assert_eq!(
        options.paths,
        vec![PathBuf::from("C:\\Projects"), PathBuf::from("D:\\Archive")]
    );
}

#[test]
fn normalizes_virtual_paths_before_directory_validation() {
    let options = parse_args(["EdenExplorer.exe", "file:\\", "shell:RecycleBinFolder"]).unwrap();
    assert_eq!(
        existing_directories(&options).unwrap(),
        vec![
            PathBuf::from(MY_PC_PATH),
            PathBuf::from(MY_RECYCLE_BIN_PATH)
        ]
    );
}

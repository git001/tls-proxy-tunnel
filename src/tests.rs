use super::*;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// parse_args: no args → Run { config_path: None }
#[test]
fn test_parse_args_empty() {
    assert!(matches!(
        parse_args(&s(&[])),
        Ok(Cli::Run { config_path: None })
    ));
}

// parse_args: --help → Help
#[test]
fn test_parse_args_help_long() {
    assert!(matches!(parse_args(&s(&["--help"])), Ok(Cli::Help)));
}

// parse_args: -h → Help
#[test]
fn test_parse_args_help_short() {
    assert!(matches!(parse_args(&s(&["-h"])), Ok(Cli::Help)));
}

// parse_args: --config path → Run { config_path: Some(path) }
#[test]
fn test_parse_args_config_long() {
    let result = parse_args(&s(&["--config", "/etc/tpt.yaml"]));
    assert!(matches!(
        result,
        Ok(Cli::Run { config_path: Some(ref p) }) if p == "/etc/tpt.yaml"
    ));
}

// parse_args: -c path → Run { config_path: Some(path) }
#[test]
fn test_parse_args_config_short() {
    let result = parse_args(&s(&["-c", "my.yaml"]));
    assert!(matches!(
        result,
        Ok(Cli::Run { config_path: Some(ref p) }) if p == "my.yaml"
    ));
}

// parse_args: --config without path → Err
#[test]
fn test_parse_args_config_missing_path() {
    assert!(parse_args(&s(&["--config"])).is_err());
}

// parse_args: unknown argument → Err
#[test]
fn test_parse_args_unknown_arg() {
    let err = parse_args(&s(&["--foo"])).unwrap_err();
    assert!(err.contains("--foo"));
}

// find_config: TPT_CONFIG env var points to existing file → Ok
#[test]
fn test_find_config_env_var() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    unsafe { std::env::set_var("TPT_CONFIG", &path) };
    let result = find_config();
    unsafe { std::env::remove_var("TPT_CONFIG") };
    assert_eq!(result.unwrap(), path);
}

// find_config: no env var, no file on disk → Err with tried paths
#[test]
fn test_find_config_not_found() {
    unsafe { std::env::remove_var("TPT_CONFIG") };
    let tmp = tempfile::TempDir::new().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let result = find_config();
    std::env::set_current_dir(original).unwrap();
    assert!(result.is_err());
    assert!(!result.unwrap_err().is_empty());
}

// run: bad arg → Err(1)
#[test]
fn test_run_unknown_arg() {
    assert_eq!(run(&s(&["--unknown"])), Err(1));
}

// run: --help → Ok (no exit)
#[test]
fn test_run_help() {
    assert_eq!(run(&s(&["--help"])), Ok(()));
}

// run: -c with non-existent file → Err(1) (config load fails)
#[test]
fn test_run_config_not_found() {
    assert_eq!(run(&s(&["-c", "/nonexistent/path/tpt.yaml"])), Err(1));
}

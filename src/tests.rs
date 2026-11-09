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

// find_config_in: $TPT_CONFIG points to an existing file → Ok
#[test]
fn test_find_config_env_var() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_string_lossy().to_string();
    assert_eq!(find_config_in(Some(path.clone()), &[]).unwrap(), path);
}

// find_config_in: $TPT_CONFIG wins over a config file in a search dir
#[test]
fn test_find_config_env_var_first() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("tpt.yaml"), "").unwrap();
    let env = tempfile::NamedTempFile::new().unwrap();
    let env_path = env.path().to_string_lossy().to_string();
    let d = dir.path().to_string_lossy().to_string();
    assert_eq!(
        find_config_in(Some(env_path.clone()), &[&d]).unwrap(),
        env_path
    );
}

// find_config_in: tpt.yaml before config.yaml in the same dir
#[test]
fn test_find_config_dir_order() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("config.yaml"), "").unwrap();
    let d = dir.path().to_string_lossy().to_string();
    assert!(
        find_config_in(None, &[&d])
            .unwrap()
            .ends_with("config.yaml")
    );
    std::fs::write(dir.path().join("tpt.yaml"), "").unwrap();
    assert!(find_config_in(None, &[&d]).unwrap().ends_with("tpt.yaml"));
}

// find_config_in: nothing found → Err with every tried path
#[test]
fn test_find_config_not_found() {
    let dir = tempfile::TempDir::new().unwrap();
    let d = dir.path().to_string_lossy().to_string();
    let tried = find_config_in(Some("/nonexistent/tpt.yaml".into()), &[&d]).unwrap_err();
    assert_eq!(tried.len(), 3, "{tried:?}");
    assert_eq!(tried[0], "/nonexistent/tpt.yaml");
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

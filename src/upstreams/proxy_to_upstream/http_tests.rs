use super::*;

// --- parse_connect_status ---

#[test]
fn test_parse_connect_status_valid() {
    assert_eq!(
        parse_connect_status(b"HTTP/1.1 200 Connection established\r\n\r\n").unwrap(),
        200
    );
}

#[test]
fn test_parse_connect_status_201() {
    assert_eq!(
        parse_connect_status(b"HTTP/1.1 201 Created\r\n\r\n").unwrap(),
        201
    );
}

#[test]
fn test_parse_connect_status_407() {
    assert_eq!(
        parse_connect_status(b"HTTP/1.1 407 Proxy Authentication Required\r\n").unwrap(),
        407
    );
}

#[test]
fn test_parse_connect_status_no_whitespace() {
    assert!(parse_connect_status(b"GARBAGE").is_err());
}

#[test]
fn test_parse_connect_status_nonnumeric_status() {
    assert!(parse_connect_status(b"HTTP/1.1 abc OK\r\n").is_err());
}

// --- resolve_header_value ---

#[test]
fn test_resolve_header_value_no_dollar() {
    let result = resolve_header_value("Basic abc123").unwrap();
    assert_eq!(result, "Basic abc123");
}

#[test]
fn test_resolve_header_value_lone_dollar() {
    let result = resolve_header_value("$").unwrap();
    assert_eq!(result, "$");
}

#[test]
fn test_resolve_header_value_with_var() {
    unsafe { std::env::set_var("TPT_TEST_RESOLVE_VAR", "secret") };
    let result = resolve_header_value("Bearer $TPT_TEST_RESOLVE_VAR").unwrap();
    assert_eq!(result, "Bearer secret");
}

#[test]
fn test_resolve_header_value_missing_var() {
    unsafe { std::env::remove_var("TPT_TEST_MISSING_VAR") };
    assert!(resolve_header_value("$TPT_TEST_MISSING_VAR").is_err());
}

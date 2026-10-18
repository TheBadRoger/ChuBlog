use super::*;
#[test]
fn timestamps_use_utc_seconds_at_the_database_boundary() {
    assert_eq!(
        to_timestamp("2026-10-17T12:34:56.789+08:00").unwrap(),
        "2026-10-17 04:34:56"
    );
    assert_eq!(
        from_timestamp("2026-10-17 04:34:56").unwrap(),
        "2026-10-17T04:34:56Z"
    );
    assert!(to_timestamp("").is_err());
    assert!(from_timestamp("2026-99-99 00:00:00").is_err());
}
#[test]
fn sql_scripts_preserve_literals_and_reject_unfinished_input() {
    let result = statements("-- setup\nCREATE TABLE t (x TEXT); /* a /* nested */ b */ INSERT INTO t VALUES ('a;''b--c');").unwrap();
    assert_eq!(result.len(), 2);
    assert!(result[1].contains("'a;''b--c'"));
    assert!(statements("SELECT 'oops").is_err());
    assert!(statements("/* oops").is_err());
    assert_eq!(statements(ACCOUNTS_SQL).unwrap().len(), 9);
}

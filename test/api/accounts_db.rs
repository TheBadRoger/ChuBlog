use super::*;
use argon2::PasswordHasher;

#[test]
fn permission_labels_decode_without_integer_or_unknown_fallback() {
    let salt = argon2::password_hash::SaltString::encode_b64(b"account-test-salt").unwrap();
    let hash = argon2::Argon2::default()
        .hash_password(b"Test-password-123", &salt)
        .unwrap()
        .to_string();
    let mut row = vec![
        Value::from(1),
        Value::from("alice"),
        Value::from(hash),
        Value::from("2026-01-01 00:00:00"),
        Value::Null,
        Value::Null,
        Value::from("测试昵称"),
    ];
    for level in [Level::Banned, Level::Member, Level::Author, Level::Master] {
        row[5] = Value::from(level.as_str());
        let user = decode(&row).unwrap();
        assert_eq!(user.level, level);
        assert_eq!(user.view()["permission_level"], level.as_str());
        assert!(user.view().get("password_hash").is_none());
        assert_eq!(
            Level::from_grant(level.grant().enabled, level.grant().role),
            level
        );
    }
    for invalid in [
        Value::from(0),
        Value::from(3),
        Value::from("member"),
        Value::from("UNKNOWN"),
        Value::Null,
    ] {
        row[5] = invalid;
        assert!(decode(&row).is_err());
    }
}

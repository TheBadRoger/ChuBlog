use super::*;
#[tokio::test]
async fn installation_token_survives_restart_and_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let first = load_token(dir.path()).await.unwrap();
    assert_eq!(first.len(), 43);
    assert_eq!(load_token(dir.path()).await.unwrap(), first);
    assert_eq!(
        std::fs::metadata(dir.path().join("setup-token"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
#[test]
fn installation_input_limits_and_token_validation() {
    assert!(
        Site {
            name: "我的博客".into(),
            description: "文章与随笔。".into()
        }
        .validate()
        .is_ok()
    );
    for name in ["".into(), "x".repeat(121), "bad\nname".into()] {
        assert!(
            Site {
                name,
                description: String::new()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        Site {
            name: "blog".into(),
            description: "x".repeat(1001)
        }
        .validate()
        .is_err()
    );
    assert!(matches_token("secret", "secret"));
    assert!(!matches_token("secret", "secrex"));
    assert!(!matches_token("secret", "secre"));
}

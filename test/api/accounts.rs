use super::*;
#[test]
fn password_limits_count_unicode_characters() {
    for value in [
        "123456".to_owned(),
        "a".repeat(128),
        "密".repeat(6),
        "😺".repeat(128),
    ] {
        assert!(password(&value).is_ok());
    }
    for value in ["short".to_owned(), "a".repeat(129), "😺".repeat(129)] {
        assert!(password(&value).is_err());
    }
}
#[test]
fn database_errors_report_safe_categories_instead_of_one_outage_message() {
    for (code, detail, expected) in [
        ("forbidden", "administrator required", "权限配置异常"),
        (
            "unauthorized",
            "secret credential detail",
            "连接凭据校验失败",
        ),
        (
            "query_error",
            "unknown column: nickname",
            "结构与当前版本不一致",
        ),
    ] {
        let error = database_error(crate::accounts_db::Error::Database(
            chusql_driver::Error::Server {
                code: code.into(),
                message: detail.into(),
            }
            .into(),
        ));
        assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
        assert!(error.1.contains(expected));
        assert!(!error.1.contains(detail));
    }
}

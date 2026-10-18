use super::*;
#[tokio::test]
async fn starter_articles_are_safe_owned_and_retryable_without_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let repo = git_store::init_author(dir.path(), "owner").await.unwrap();
    let commit = write_repository(&repo, "owner").await.unwrap();
    assert_eq!(write_repository(&repo, "owner").await.unwrap(), commit);
    assert_eq!(
        text(&repo, &["show", "-s", "--format=%an", "HEAD"], None)
            .await
            .unwrap(),
        "owner"
    );
    let snapshot = match content::scan_at(&repo, &commit).await.unwrap() {
        content::Scan::Ready(snapshot) => snapshot,
        content::Scan::Pending(reason) => panic!("{reason}"),
    };
    assert_eq!(snapshot.posts.len(), 2);
    assert!(
        snapshot
            .posts
            .values()
            .all(|post| post.meta.status == content::Status::Published)
    );
    let data = dir.path().join("data");
    let combined = git_store::import_authors(
        &data,
        dir.path(),
        &content::Snapshot::default(),
        &["owner".into()],
    )
    .await
    .unwrap();
    assert!(combined.posts.contains_key("owner~publishing"));
    assert!(combined.posts.contains_key("owner~development"));
    assert!(combined.posts.values().all(|post| post.author == "owner"));
    content::git(&repo, &["read-tree", "--empty"], None)
        .await
        .unwrap();
    let tree = text(&repo, &["write-tree"], None).await.unwrap();
    let next = text(
        &repo,
        &["commit-tree", &tree, "-p", &commit, "-m", "Owner changes"],
        None,
    )
    .await
    .unwrap();
    content::git(&repo, &["update-ref", "refs/heads/main", &next], None)
        .await
        .unwrap();
    assert!(write_repository(&repo, "owner").await.is_err());
    assert_eq!(
        text(&repo, &["rev-parse", "HEAD"], None).await.unwrap(),
        next
    );
}

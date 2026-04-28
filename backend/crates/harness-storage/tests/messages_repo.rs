//! Behavior tests for `SqliteMessageRepo`.

use harness_core::ids::{ConversationId, ProviderId};
use harness_core::message::{ContentBlock, Role};
use harness_core::repo::{ConversationRepo, MessageRepo, NewConversation, NewMessage};
use harness_core::RepoError;
use harness_storage::{
    Db, Secret, SqliteConversationRepo, SqliteMessageRepo,
};
use tempfile::NamedTempFile;

async fn open_db() -> (NamedTempFile, Db) {
    let tmp = NamedTempFile::new().expect("tempfile");
    let db = Db::open(tmp.path(), Secret::from_bytes([1u8; 32]))
        .await
        .expect("open db");
    (tmp, db)
}

async fn make_conversation(db: &Db) -> ConversationId {
    let repo = SqliteConversationRepo::new(db.clone());
    repo.create(NewConversation {
        title: "T".into(),
        provider_id: ProviderId::from_string("claude"),
        model: "m".into(),
        sandbox_template_id: None,
    })
    .await
    .unwrap()
    .id
}

fn text(role: Role, s: &str, conv: &ConversationId) -> NewMessage {
    NewMessage {
        conversation_id: conv.clone(),
        role,
        content: vec![ContentBlock::Text { text: s.into() }],
    }
}

#[tokio::test]
async fn append_round_trips_with_content() {
    let (_tmp, db) = open_db().await;
    let conv = make_conversation(&db).await;
    let repo = SqliteMessageRepo::new(db);

    let stored = repo.append(text(Role::User, "hello", &conv)).await.unwrap();
    assert_eq!(stored.role, Role::User);
    assert_eq!(stored.conversation_id, conv);
    assert_eq!(stored.ordinal, 0, "first ordinal must be 0");

    let fetched = repo.get(&stored.id).await.unwrap();
    assert_eq!(fetched, stored);
}

#[tokio::test]
async fn ordinals_are_monotonic_per_conversation() {
    let (_tmp, db) = open_db().await;
    let conv_a = make_conversation(&db).await;
    let conv_b = make_conversation(&db).await;
    let repo = SqliteMessageRepo::new(db);

    let a0 = repo.append(text(Role::User, "a0", &conv_a)).await.unwrap();
    let a1 = repo
        .append(text(Role::Assistant, "a1", &conv_a))
        .await
        .unwrap();
    let a2 = repo.append(text(Role::User, "a2", &conv_a)).await.unwrap();

    assert_eq!((a0.ordinal, a1.ordinal, a2.ordinal), (0, 1, 2));

    // A separate conversation has its own ordinal sequence starting at 0.
    let b0 = repo.append(text(Role::User, "b0", &conv_b)).await.unwrap();
    assert_eq!(b0.ordinal, 0);
}

#[tokio::test]
async fn list_returns_messages_in_ordinal_order() {
    let (_tmp, db) = open_db().await;
    let conv = make_conversation(&db).await;
    let repo = SqliteMessageRepo::new(db);

    let m0 = repo.append(text(Role::User, "0", &conv)).await.unwrap();
    let m1 = repo.append(text(Role::Assistant, "1", &conv)).await.unwrap();
    let m2 = repo.append(text(Role::Tool, "2", &conv)).await.unwrap();

    let listed = repo.list(&conv).await.unwrap();
    assert_eq!(listed, vec![m0, m1, m2]);
}

#[tokio::test]
async fn append_to_unknown_conversation_yields_conflict() {
    let (_tmp, db) = open_db().await;
    let repo = SqliteMessageRepo::new(db);

    let err = repo
        .append(text(
            Role::User,
            "hi",
            &ConversationId::from_string("nope"),
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, RepoError::Conflict(_)), "got {err:?}");
}

#[tokio::test]
async fn deleting_conversation_cascades_messages() {
    let (_tmp, db) = open_db().await;
    let conv = make_conversation(&db).await;
    let conv_repo = SqliteConversationRepo::new(db.clone());
    let repo = SqliteMessageRepo::new(db);

    repo.append(text(Role::User, "hi", &conv)).await.unwrap();
    repo.append(text(Role::Assistant, "yo", &conv))
        .await
        .unwrap();
    assert_eq!(repo.list(&conv).await.unwrap().len(), 2);

    conv_repo.delete(&conv).await.unwrap();
    assert!(repo.list(&conv).await.unwrap().is_empty());
}

#[tokio::test]
async fn structured_content_round_trips() {
    let (_tmp, db) = open_db().await;
    let conv = make_conversation(&db).await;
    let repo = SqliteMessageRepo::new(db);

    let blocks = vec![
        ContentBlock::Text {
            text: "thinking…".into(),
        },
        ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "echo".into(),
            input: serde_json::json!({"text": "hi"}),
        },
    ];
    let stored = repo
        .append(NewMessage {
            conversation_id: conv.clone(),
            role: Role::Assistant,
            content: blocks.clone(),
        })
        .await
        .unwrap();

    assert_eq!(stored.content, blocks);

    let fetched = repo.get(&stored.id).await.unwrap();
    assert_eq!(fetched.content, blocks);
}

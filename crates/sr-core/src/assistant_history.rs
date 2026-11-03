//! Persistent assistant conversations.
//!
//! One JSON file per conversation under `<app data>/assistant/chats/`. Messages
//! store the exact text plus tool traces/proposals so the UI can restore cards.

use crate::error::{AppError, Result};
use crate::paths::AppPaths;
use crate::util;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub traces: Vec<Value>,
    #[serde(default)]
    pub proposals: Vec<Value>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub instance_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<StoredMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub instance_id: Option<String>,
    pub updated_at: String,
    pub message_count: usize,
}

fn chats_dir(paths: &AppPaths) -> PathBuf {
    paths.base().join("assistant").join("chats")
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn new_conversation(instance_id: Option<String>) -> Conversation {
    let now = util::now_rfc3339();
    Conversation {
        id: util::short_id(),
        title: "新对话".into(),
        instance_id,
        created_at: now.clone(),
        updated_at: now,
        messages: Vec::new(),
    }
}

pub async fn list(paths: &AppPaths) -> Result<Vec<ConversationSummary>> {
    let dir = chats_dir(paths);
    let mut out = Vec::new();
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = rd.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") {
            continue;
        }
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if !safe_id(id) {
            continue;
        }
        if let Ok(Some(conv)) = util::read_json::<Conversation>(&entry.path()).await {
            out.push(ConversationSummary {
                id: conv.id,
                title: conv.title,
                instance_id: conv.instance_id,
                updated_at: conv.updated_at,
                message_count: conv.messages.len(),
            });
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

pub async fn load(paths: &AppPaths, id: &str) -> Result<Conversation> {
    if !safe_id(id) {
        return Err(AppError::invalid_input("对话 ID 非法"));
    }
    util::read_json::<Conversation>(&chats_dir(paths).join(format!("{id}.json")))
        .await?
        .ok_or_else(|| AppError::not_found("对话不存在"))
}

pub async fn save(paths: &AppPaths, conversation: &Conversation) -> Result<()> {
    if !safe_id(&conversation.id) {
        return Err(AppError::invalid_input("对话 ID 非法"));
    }
    util::write_json(
        &chats_dir(paths).join(format!("{}.json", conversation.id)),
        conversation,
    )
    .await
}

pub async fn delete(paths: &AppPaths, id: &str) -> Result<()> {
    if !safe_id(id) {
        return Err(AppError::invalid_input("对话 ID 非法"));
    }
    let path = chats_dir(paths).join(format!("{id}.json"));
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        tokio::fs::remove_file(&path).await?;
    }
    Ok(())
}

pub async fn delete_all(paths: &AppPaths) -> Result<usize> {
    let dir = chats_dir(paths);
    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    let mut removed = 0;
    while let Some(entry) = rd.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".json") {
            let _ = tokio::fs::remove_file(entry.path()).await;
            removed += 1;
        }
    }
    Ok(removed)
}

pub fn title_from(message: &str) -> String {
    let one_line = message
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(message)
        .trim();
    let mut title: String = one_line.chars().take(40).collect();
    if one_line.chars().count() > 40 {
        title.push('…');
    }
    if title.is_empty() {
        "新对话".into()
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn saves_lists_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path());
        paths.ensure_layout().await.unwrap();
        let mut conv = new_conversation(None);
        conv.title = title_from("分析这个崩溃\n第二行");
        conv.messages.push(StoredMessage {
            role: "user".into(),
            content: "hi".into(),
            traces: Vec::new(),
            proposals: Vec::new(),
            created_at: util::now_rfc3339(),
        });
        save(&paths, &conv).await.unwrap();
        let rows = list(&paths).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "分析这个崩溃");
        assert_eq!(load(&paths, &conv.id).await.unwrap().messages.len(), 1);
        delete(&paths, &conv.id).await.unwrap();
        assert!(list(&paths).await.unwrap().is_empty());
    }
}

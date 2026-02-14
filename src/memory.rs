use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use uuid::Uuid;

use crate::{DEV, get_logger, objects::Message};


pub struct MemoryService {
    pool: PgPool
}

impl MemoryService {
    pub async fn init() -> anyhow::Result<Self> {
        let database_url =
            std::env::var("DATABASE_URL")
                .unwrap_or("postgres://bot:your_strong_password@localhost:5432/botdb".to_string());

        let pool =  PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&database_url)
            .await?;

        let service = Self { pool };
        service.init_schema().await?;

        Ok(service)
    }

    pub async fn init_schema(&self) -> anyhow::Result<()> {
        let logger = get_logger();
        
        if DEV {
            logger.warn("Dev mode: Dropping memories table...");
            sqlx::query("DROP TABLE IF EXISTS memories CASCADE;")
                .execute(&self.pool)
                .await?;
            logger.warn("Memories table removed.");
        }

        logger.info("Ensuring schema...");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memories (
                id UUID PRIMARY KEY,
                scope TEXT NOT NULL,
                category TEXT NOT NULL,
                key TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata JSONB,
                created_at TIMESTAMPTZ DEFAULT now(),
                updated_at TIMESTAMPTZ DEFAULT now(),
                UNIQUE(scope, key)
            )
            "#
        ).execute(&self.pool).await?;

        // 创建索引 - 分别执行每条语句
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_memories_scope ON memories(scope);"
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_memories_category ON memories(category);"
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_memories_key ON memories(key);"
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_memories_metadata ON memories USING GIN(metadata);"
        )
        .execute(&self.pool)
        .await?;

        logger.info("Schema ready.");

        Ok(())
    }

    pub async fn upsert(
        &self,
        scope: Scope,
        category: &str,
        key: &str,
        content: &str,
        metadata: Option<Value>,
    ) -> anyhow::Result<()> {

        sqlx::query(
            r#"
            INSERT INTO memories (id, scope, category, key, content, metadata)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (scope, key)
            DO UPDATE SET
                category = EXCLUDED.category,
                content = EXCLUDED.content,
                metadata = EXCLUDED.metadata,
                updated_at = now()
            "#
        )
        .bind(Uuid::new_v4())
        .bind(scope.to_string())
        .bind(category)
        .bind(key)
        .bind(content)
        .bind(metadata)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get(
        &self,
        scope: Scope,
        key: &str,
    ) -> anyhow::Result<Option<Memory>> {

        let row = sqlx::query(
            r#"
            SELECT id, scope, category, key, content,
                   metadata, created_at, updated_at
            FROM memories
            WHERE scope = $1 AND key = $2
            "#
        )
        .bind(scope.to_string())
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(Memory {
                id: row.try_get("id")?,
                scope: Scope::from(row.try_get::<String, _>("scope")?),
                category: row.try_get("category")?,
                key: row.try_get("key")?,
                content: row.try_get("content")?,
                metadata: row.try_get("metadata")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn search(
        &self,
        scope: Scope,
        keyword: &str,
    ) -> anyhow::Result<Vec<Memory>> {

        let pattern = format!("%{}%", keyword);

        let rows = sqlx::query(
            r#"
            SELECT id, scope, category, key, content,
                   metadata, created_at, updated_at
            FROM memories
            WHERE scope = $1
              AND (key ILIKE $2 OR metadata::text ILIKE $3)
            ORDER BY updated_at DESC
            "#
        )
        .bind(scope.to_string())
        .bind(pattern.clone())
        .bind(pattern)
        .fetch_all(&self.pool)
        .await?;

        let mut memories = Vec::new();

        for row in rows {
            memories.push(Memory {
                id: row.try_get("id")?,
                scope: Scope::from(row.try_get::<String, _>("scope")?),
                category: row.try_get("category")?,
                key: row.try_get("key")?,
                content: row.try_get("content")?,
                metadata: row.try_get("metadata")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            });
        }

        Ok(memories)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum Scope {
    Group(usize),
    User(usize),
    Global
}

impl ToString for Scope {
    fn to_string(&self) -> String {
        match self {
            Scope::Global => "global".to_string(),
            Scope::Group(group_id) => format!("group:{}", group_id),
            Scope::User(user_id) => format!("user:{}", user_id)
        }
    }
}

impl From<String> for Scope {
    fn from(value: String) -> Self {
        if value == "global" {
            Scope::Global
        } else if let Some(id_str) = value.strip_prefix("group:") {
            if let Ok(id) = id_str.parse::<usize>() {
                Scope::Group(id)
            } else {
                Scope::Global
            }
        } else if let Some(id_str) = value.strip_prefix("user:") {
            if let Ok(id) = id_str.parse::<usize>() {
                Scope::User(id)
            } else {
                Scope::Global
            }
        } else {
            Scope::Global
        }
    }
}

impl From<&Message> for Scope {
    fn from(value: &Message) -> Self {
        if value.private {
            Scope::User(value.sender.user_id)
        } else {
            if let Some(group) = &value.group {
                Scope::Group(group.group_id)
            } else {
                Scope::Global
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Memory {
    pub id: Uuid,
    pub scope: Scope,
    pub category: String,
    pub key: String,
    pub content: String,
    pub metadata: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>
}

impl Memory {
    pub fn format(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("category".to_string(), self.category.clone().into());
        map.insert("key".to_string(), self.key.clone().into());
        map.insert("content".to_string(), self.content.clone().into());
        if let Some(metadata) = &self.metadata {
            map.insert("metadata".to_string(), metadata.clone());
        };
        Value::Object(map)
    }
}
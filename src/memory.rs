use std::{collections::HashMap, sync::Arc, time::Duration, usize};

use chrono::{DateTime, Utc};
use deepseek_api::{CompletionsRequestBuilder, DeepSeekClient, RequestBuilder, request::{MessageRequest, ToolObject, UserMessageRequest}, response::ModelType};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};

use crate::{DEV, get_logger, objects::{Group, Message, Permission, User}, self_id, tools::{AddMemoryTool, DeleteMemoryTool, ToolRegistry, UpdateMemoryTool}};

pub struct Dozer {
    pub temp: HashMap<Scope, Vec<Message>>,
    pub mem_service: Arc<MemoryService>,
    pub mem_tools: ToolRegistry,
}

impl Dozer {
    pub fn new(service: Arc<MemoryService>) -> Self {

        let mut tools = ToolRegistry::new();
        tools.register(UpdateMemoryTool { service: service.clone() });
        tools.register(AddMemoryTool { service: service.clone() });
        tools.register(DeleteMemoryTool { service: service.clone() });

        Self { 
            temp: HashMap::new(),
            mem_service: service,
            mem_tools: tools,
        }
    }

    pub fn temp(&mut self, msg: Message) {
        let scope = Scope::from(&msg);
        if let Some(msgs) = self.temp.get_mut(&scope) {
            msgs.push(msg);
        } else {
            let msgs = vec![msg];
            self.temp.insert(scope, msgs);
        }
    }

    pub async fn doze(&mut self, client: &DeepSeekClient) -> anyhow::Result<()> {

        let mut to_process = Vec::new();
        let mut to_keep = Vec::new();
        
        let threshold = if DEV { 1 } else { 50 };

        for (scope, temped_msgs) in self.temp.drain() {
            if temped_msgs.len() >= threshold {
                to_process.push((scope, temped_msgs));
            } else {
                to_keep.push((scope, temped_msgs));
            }
        }

        for (scope, msgs) in to_keep {
            self.temp.insert(scope, msgs);
        }

        for (scope, msgs) in to_process {
            let formatted = self.format_msgs(&msgs)?;
            self.mem_event(scope, formatted, client).await?;
        }

        Ok(())
    }

    pub async fn mem_event(&self, scope: Scope, msgs: String, client: &DeepSeekClient) -> anyhow::Result<()> {

        let prompt = format!(r#"
你是一个“聊天记录关键信息提取器”。

你的任务：
从给定的聊天记录中，提取“长期有价值或值得记忆的关键信息”。

关键信息包括但不限于：
- 用户的身份、职业、兴趣、技能
- 明确的事实陈述
- 重要事件或决定
- 持续性的偏好或状态
- 明确的计划或目标

不要提取：
- 寒暄
- 无意义回复
- 情绪性短句
- 上下文依赖严重的内容

--------------------------------
输出格式（必须严格遵守）：

每条信息单独一行，使用 JSON Lines 格式：
每一行必须是一个完整 JSON 对象。

格式如下：
{{"info":"提取出的关键信息句子"}}

禁止输出任何解释、前缀、Markdown、代码块或额外文本。
提取别称的输出规则见工具说明。
--------------------------------
规则：
1. 每条 info 必须是“完整独立句子”
2. 使用第三人称客观描述
3. 使用用户id（纯数字）代称用户
4. 不要重复信息，不要有遗漏信息
6. 如果没有重要信息，请输出 `NO_RESPONSE`（不要解释）
--------------------------------
聊天记录：

{}
        "#, msgs);

        get_logger().debug(&msgs);

        let resp = CompletionsRequestBuilder::new(&vec![
            MessageRequest::User(UserMessageRequest { content: prompt, name: None })
        ]).use_model(ModelType::DeepSeekChat).do_request(client).await?.must_response();

        if let Some(choice) = resp.choices.first() {
            if let Some(assistant_msg) = &choice.message {
                if !(assistant_msg.content.contains("NO_RESPONSE") && assistant_msg.content.len() < 20) {

                    for info in assistant_msg.content.lines() {
                        println!("{}", info);

                        if let Ok(info) = serde_json::from_str::<Value>(info) {
                            if let Some(info_str) = info.get("info").and_then(|v| v.as_str()) {

                                let mut prompt = Vec::new();
                                prompt.push("过去的记忆：".to_string());
                                for mem in self.mem_service.similars(scope, info_str).await? {
                                    prompt.push(mem.format().to_string());
                                }
                                prompt.push("".to_string());
                                prompt.push("新的记忆：".to_string());
                                prompt.push(assistant_msg.content.to_string());
                                prompt.push("".to_string());
                                prompt.push(r#"
说明：
请将新的记忆与旧的记忆比对分析。
如果新记忆与旧记忆发生矛盾或对旧记忆产生否定，以新的记忆为准，调用 `update_memory` 工具，订正记忆，##删除错误记忆##，用新记忆取代，并降低confidence;
如果新记忆可以对旧记忆做出补充和证明，调用 `update_memory` 工具，更新记忆，并适当提高confidence;
如果旧记忆之间关联性很大，应当将信息较少的记忆整合到信息较多的记忆中去，并调用 `delete_memory` 工具，删除被整合的短记忆;
注意：不要提到新旧记忆的关系，仅对内容做出覆盖更新。
如果旧记忆为空或没有与新记忆相似的信息，调用 `add_memory` 工具，将新记忆作为一条全新记忆存储;
如果新记忆中没有有价值的信息，你可以选择不调用工具，但不建议你这样做，因为信息已经经过筛选。
                                "#.to_string());

                                let tools = self.mem_tools.format_for_openai_api().iter().map(|tool| {
                                    serde_json::from_value::<ToolObject>(tool.clone())
                                }).collect::<Result<Vec<ToolObject>, _>>()?;

                                let resp = CompletionsRequestBuilder::new(&vec![
                                    MessageRequest::User(UserMessageRequest { content: prompt.join("\n"), name: None })
                                ]).use_model(ModelType::DeepSeekChat).tools(&tools).do_request(client).await?.must_response();

                                if let Some(choice) = resp.choices.first() {
                                    if let Some(assistant_msg) = &choice.message {
                                        if let Some(tool_calls) = &assistant_msg.tool_calls {
                                            for call in tool_calls {
                                                let _ = self.mem_tools.execute_str_with_err(
                                                    &call.function.name,
                                                    &call.id,
                                                    &call.function.arguments,
                                                    &scope.try_into()?
                                                ).await;    
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn format_msgs(&self, msgs: &Vec<Message>) -> anyhow::Result<String> {
        
        let mut result = Vec::<String>::new();
        
        for msg in msgs {
            result.push(if msg.sender.user_id == self_id() {
                // This will never be matched
                format!("(你|ai): {}", msg.simplified_plain())
            } else {
                format!("(user_id:{}): {}", msg.sender.user_id, msg.simplified_plain())
            });
        }

        Ok(result.join("\n"))
    }
}

macro_rules! extract {
    ($json:expr, $key:literal, $extractor:ident) => {
        $json.get($key)
            .and_then(|v| v.$extractor().map(|o| o.to_owned()))
            .ok_or_else(|| anyhow::anyhow!("Missing argument: {}", $key))?
    };
}

pub struct MemoryService {
    pool: PgPool,
    client: Client
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

        let service = Self {
            pool: pool,
            client: ClientBuilder::new()
                .timeout(Duration::from_secs(10)).build()?
        };
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
            "CREATE EXTENSION IF NOT EXISTS vector;"
        ).execute(&self.pool).await?;

        sqlx::query(
            "CREATE EXTENSION IF NOT EXISTS pg_trgm;"
        ).execute(&self.pool).await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memories (
                id SERIAL PRIMARY KEY,
                scope TEXT NOT NULL,
                content TEXT NOT NULL,
                embedding VECTOR(1024),
                tsv tsvector,
                confidence FLOAT DEFAULT 0.2,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                last_accessed TIMESTAMPTZ DEFAULT NOW()
            );
            "#
        ).execute(&self.pool).await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS memories_embedding_idx
            ON memories USING ivfflat (embedding vector_cosine_ops);
            "#
        ).execute(&self.pool).await?;

        sqlx::query(
            r#"
            CREATE INDEX memories_tsv_idx
            ON memories USING GIN(tsv);
            "#
        ).execute(&self.pool).await?;

        logger.info("Schema ready.");

        Ok(())
    }

    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let resp = self.client.post(std::env::var("EMBED_API_ROOT").expect("No embedding api root provided"))
            .header("Authorization", format!("Bearer {}", std::env::var("EMBED_API_KEY").expect("No embedding api key provided")))
            .json(&json!({
                "model": "embedding-3",
                "input": text,
                "dimensions": 1024
            }))
            .send().await?.json::<Value>().await?;
        let embedding = extract!(extract!(resp, "data", as_array).first()
            .ok_or_else(|| anyhow::anyhow!("Empty data"))?.to_owned(), "embedding", as_array)
            .iter().map(|n| n.as_f64().map(|f| f as f32).ok_or_else(|| anyhow::anyhow!("Bad f32"))).collect::<Result<Vec<f32>, _>>()?;
        Ok(embedding)
    }

    pub async fn create(
        &self,
        scope: Scope,
        content: &str,
    ) -> anyhow::Result<()> {

        sqlx::query(
            r#"
            INSERT INTO memories 
            (scope, content, embedding, tsv) 
            VALUES ($1, $2, $3, to_tsvector('simple', $2));
            "#
        )
        .bind(scope.to_string())
        .bind(content)
        .bind(self.embed(content).await?)
        .execute(&self.pool).await?;

        Ok(())
    }

    pub async fn merge(
        &self,
        id: i32,
        content: &str,
        confidence: f64
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            UPDATE memories
            SET
                content = $1,
                embedding = $2,
                confidence = $3,
                last_accessed = NOW()
            WHERE id = $4
            "#
        )
        .bind(content)
        .bind(self.embed(content).await?)
        .bind(confidence)
        .bind(id)
        .execute(&self.pool).await?;
        
        Ok(())
    }

    pub async fn delete(
        &self,
        id: i32
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            DELETE FROM memories
            WHERE id = $1
            "#
        )
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn similars(
        &self,
        scope: Scope,
        content: &str
    ) -> anyhow::Result<Vec<Memory>> {

        let rows = sqlx::query(
            r#"
            WITH similarity_scores AS (
                SELECT
                    id,
                    scope as scope_str,
                    content,
                    confidence,
                    created_at,
                    embedding <=> $1::vector(1024) AS cosine_dist,
                    ts_rank(tsv, plainto_tsquery('simple', $2)) AS text_score
                FROM memories
                WHERE scope = $3
            )
            SELECT
                id,
                scope_str,
                content,
                confidence,
                created_at,
                ((1 - cosine_dist) * 0.7 + text_score * 0.3) AS score
            FROM similarity_scores
            WHERE
                cosine_dist < 0.6 OR text_score > 0
            ORDER BY score DESC
            LIMIT 6
            "#
        )
        .bind(self.embed(content).await?)
        .bind(content)
        .bind(scope.to_string())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter()
            .map(|row| Memory {
                id: row.get("id"),
                scope: Scope::from(row.get::<String, _>("scope_str")),
                content: row.get("content"),
                confidence: row.get("confidence"),
                created_at: row.get("created_at")
            }).collect())
    }
    
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
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

impl TryInto<Message> for Scope {
    type Error = anyhow::Error;
    fn try_into(self) -> Result<Message, Self::Error> {
        match self {
            Self::Global => Err(anyhow::anyhow!("Cannot convert global Scope into Message")),
            Self::User(user_id) => Ok(Message {
                message_id: 0,
                private: true,
                group: None,
                sender: User {
                    user_id: user_id,
                    nickname: None,
                    card: None,
                    role: Permission::Normal
                },
                raw: "".to_string(),
                array: vec![]
            }),
            Self::Group(group_id) => Ok(Message {
                message_id: 0,
                private: false,
                group: Some(Group {
                    group_id: group_id,
                    group_name: None
                }),
                sender: User {
                    user_id: 0,
                    nickname: None,
                    card: None,
                    role: Permission::Normal
                },
                raw: "".to_string(),
                array: vec![]
            })
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Memory {
    pub id: i32,
    pub scope: Scope,
    pub content: String,
    pub confidence: f64,
    pub created_at: DateTime<Utc>
}

impl Memory {
    pub fn format(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("id".to_string(), self.id.clone().into());
        map.insert("content".to_string(), self.content.clone().into());
        map.insert("confidence".to_string(), self.confidence.clone().into());
        Value::Object(map)
    }

    pub fn simplified_plain(&self) -> String {
        format!("{} (置信度: {})", self.content, self.confidence)
    }
}
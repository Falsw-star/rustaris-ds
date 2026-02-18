use std::{collections::HashMap, sync::{Arc, Mutex}, time::Duration};

use rust_mc_status::{McClient, ServerEdition};
use serde_json::{Value, json};

use async_trait::async_trait;
use crate::{get_logger, get_poster, memory::{MemoryService, Scope}, objects::{Message, MessageArrayItem}, thinking::AliasesMapping};



#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn call(&self, args: Value, msg: &Message) -> anyhow::Result<Value>;
    fn parameters_schema(&self) -> Value;
}

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub async fn execute_str_with_err(
        &self,
        name: &str,
        id: &str,
        args: &str,
        msg: &Message
    ) -> Value {
        match self.execute_str(name, id, args, msg).await {
            Ok(result) => result,
            Err(err) => json!({
                "role": "tool",
                "tool_call_id": id,
                "content": format!("工具 '{}' 调用失败：{}", name, err)
            })
        }
    }

    pub async fn execute_str(
        &self,
        name: &str,
        id: &str,
        args: &str,
        msg: &Message
    ) -> anyhow::Result<Value> {
        self.execute(
            name,
            id,
            serde_json::from_str(args)
            .map_err(|err| anyhow::anyhow!("Invalid JSON args: {}", err))?,
            msg).await
    }

    pub async fn execute_with_err(
        &self,
        name: &str,
        id: &str,
        args: Value,
        msg: &Message
    ) -> Value {
        match self.execute(name, id, args, msg).await {
            Ok(result) => result,
            Err(err) => json!({
                "role": "tool",
                "tool_call_id": id,
                "content": format!("工具 '{}' 调用失败：{}", name, err)
            })
        }
    }

    pub async fn execute(
        &self,
        name: &str,
        id: &str,
        args: Value,
        msg: &Message
    ) -> anyhow::Result<Value> {
        let tool = 
            self.get(name).ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?; 
        get_logger().debug(&format!("Calling: {}", tool.name()));
        Ok(json!({
            "role": "tool",
            "tool_call_id": id,
            "content": tool.call(args, msg).await?
        }))
    }
    
    pub fn format_for_openai_api(&self) -> Vec<Value> {
        self.tools.values().map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name(),
                    "description": tool.description(),
                    "parameters": tool.parameters_schema()
                }
            })
        }).collect()
    }
}

macro_rules! extract {
    ($json:expr, $key:literal, $extractor:ident) => {
        $json.get($key)
            .and_then(|v| v.$extractor().map(|o| o.to_owned()))
            .ok_or_else(|| anyhow::anyhow!("Missing argument: {}", $key))?
    };
}

macro_rules! extract_optional {
    ($json:expr, $key:literal, $extractor:ident) => {
        $json.get($key)
            .and_then(|v| v.$extractor().map(|o| o.to_owned()))
    };
}

pub struct MCSTool {
    client: McClient
}

impl MCSTool {
    pub fn new() -> Self {
        Self {
            client: McClient::new()
                .with_timeout(Duration::from_secs(5))
                .with_max_parallel(5)
        }
    }
}

#[async_trait]
impl Tool for MCSTool {
    fn name(&self) -> &str {
        "mcstatus"
    }

    fn description(&self) -> &str {
        "查询 Minecraft 服务器状态"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "string",
                    "description": "服务器的地址"
                },
                "edition": {
                    "type": "string",
                    "enum": ["java", "bedrock"],
                    "default": "java",
                    "description": "待查服务器的版本类型"
                }
            },
            "required": ["address"]
        })
    }

    async fn call(&self, args: Value, _msg: &Message) -> anyhow::Result<Value> {
        let address = extract!(args, "address", as_str);
        let edition = extract_optional!(args, "edition", as_str).unwrap_or("java".to_string());

        let status = self.client.ping(
            &address.trim(),
            match edition.as_str() {
                "java" => ServerEdition::Java,
                "bedrock" => ServerEdition::Bedrock,
                _ => ServerEdition::Java
            }
        ).await?;

        Ok(Value::String(serde_json::to_string(&status)?))
    }
}

pub struct NeteaseMusicTool {
    client: reqwest::Client,
    api_root: String
}

impl NeteaseMusicTool {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::ClientBuilder::new()
                .timeout(Duration::from_secs(10))
                .build()?,
            api_root: std::env::var("NETEASE_API_ROOT").unwrap_or("http://192.168.3.38:8099".to_string())
        })
    }
}

#[async_trait]
impl Tool for NeteaseMusicTool {
    fn name(&self) -> &str {
        "netease_music"
    }

    fn description(&self) -> &str {
        "解析网易云音乐的歌曲并将对应信息转发到群中"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "歌曲的id，由用户直接告知或包含在歌曲分享链接`?id=`之后，由数字组成"
                },
                "quality": {
                    "type": "string",
                    "enum": ["standard", "exhigh", "lossless"],
                    "default": "standard",
                    "description": "歌曲的音质。可选值：standard(标准)，exhigh(高品)，lossless(无损)",
                },
                "send_cover": {
                    "type": "boolean",
                    "default": false,
                    "description": "是否仅向用户发送歌曲专辑封面。如果用户同时要歌曲和封面，则应调用本工具两次，分别设send_cover为true和false"
                },
                "as_file": {
                    "type": "boolean",
                    "default": true,
                    "description": "是否将歌曲作为文件发送。当用户索要原始链接时，应设为false"
                }
            },
            "required": ["id"]
        })
    }

    async fn call(&self, args: Value, msg: &Message) -> anyhow::Result<Value> {

        let id = extract!(args, "id", as_str).parse::<usize>()?;
        let quality = extract_optional!(args, "quality", as_str).unwrap_or("standard".to_string());

        let send_cover = extract_optional!(args, "send_cover", as_bool).unwrap_or(false);
        let as_file = extract_optional!(args, "as_file", as_bool).unwrap_or(true);

        let info = self.client.post(format!("{}/info", self.api_root))
            .json(&json!({
                "id": id
            })).send().await?.json::<Value>().await?;
        let name = sanitize_filename::sanitize(extract!(info, "name", as_str));

        if send_cover {
            let cover_url = extract!(extract!(info, "album", as_object), "cover_url", as_str);
            if msg.quick_send_msg(vec![MessageArrayItem::Image { summary: None, file: None, url: cover_url, file_size: None }]).await {
                return Ok(Value::String(format!("发送 {} 成功", name)));
            } else {
                return Ok(Value::String(format!("发送 {} 失败", name)));
            }
        }

        let audio = self.client.post(format!("{}/audio", self.api_root))
            .json(&json!({
                "id": id,
                "quality": quality
            })).send().await?.json::<Value>().await?;
        let url = extract!(audio, "url", as_str);
        let encoding = extract!(audio, "encoding", as_str);
        let file_name = format!("{}.{}", name, encoding);

        let send_result = if as_file {
            if msg.private {
                match get_poster().upload_private_file(msg.sender.user_id, &url, &file_name).await {
                    Ok(_id) => format!("发送 {} 成功", file_name),
                    Err(err) => format!("发送 {} 失败: {}", file_name, err.to_string())
                }
            } else {
                if let Some(group) = &msg.group {
                    match get_poster().upload_group_file(group.group_id, &url, &file_name).await {
                        Ok(_id) => format!("发送 {} 成功", file_name),
                        Err(err) => format!("发送 {} 失败: {}", file_name, err.to_string())
                    }
                } else { "Missing group".to_string() }
            }
        } else {
            if msg.quick_send_text(&format!("Song: {}\nurl: {}", file_name, url)).await {
                format!("发送 {} 成功", file_name)
            } else {
                format!("发送 {} 失败", file_name)
            }
        };

        Ok(Value::String(send_result))
    }
}

pub struct SearchNeteaseMusicTool {
    client: reqwest::Client,
    api_root: String
}

impl SearchNeteaseMusicTool {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::ClientBuilder::new()
                .timeout(Duration::from_secs(10))
                .build()?,
            api_root: std::env::var("NETEASE_API_ROOT").unwrap_or("http://192.168.3.38:8099".to_string())
        })
    }
}

#[async_trait]
impl Tool for SearchNeteaseMusicTool {
    fn name(&self) -> &str {
        "search_music"
    }

    fn description(&self) -> &str {
        "通过关键词搜索网易云的歌曲并返回信息"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keyword": {
                    "type": "string",
                    "description": "要搜索的关键词，可以是歌曲名/音乐风格类型/专辑名"
                }
            },
            "required": ["keyword"]
        })
    }

    async fn call(&self, args: Value, _msg: &Message) -> anyhow::Result<Value> {
        let keyword = extract!(args, "keyword", as_str);
        let limit: usize = 5;

        let array = self.client.post(format!("{}/search", self.api_root))
            .json(&json!({
                "keyword": keyword,
                "limit": limit
            })).send().await?.json::<Vec<Value>>().await?;

        let mut result = Vec::<String>::new();

        result.push(format!("找到 {} 个结果（最多 5 个结果）：", array.len()));
        
        for item in &array {
            let mut song_info = Vec::<String>::new();

            let name = extract!(item, "name", as_str);
            song_info.push(format!("name: {}", name));
            let song_id = extract!(item, "id", as_u64).to_string();
            song_info.push(format!("id: {}", song_id));
            let mut artists = Vec::<String>::new();
            for artist in extract!(item, "artists", as_array) {
                artists.push(extract!(artist, "name", as_str));
            }
            song_info.push(format!("artists: {}", artists.join(", ")));
            let album_name = extract!(extract!(item, "album", as_object), "name", as_str);
            song_info.push(format!("album: {}", album_name));

            result.push(song_info.join("\n"));
        }
        
        Ok(Value::String(result.join("\n\n")))
    }
}

pub struct UpdateMemoryTool {
    pub service: Arc<MemoryService>
}

#[async_trait]
impl Tool for UpdateMemoryTool {
    fn name(&self) -> &str {
        "update_memory"
    }

    fn description(&self) -> &str {
        "更新本条记忆"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "memories": {
                    "type": "array",
                    "description": "要更新的记忆列表",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "integer",
                                "description": "记忆ID"
                            },
                            "content": {
                                "type": "string",
                                "description": "更新后的记忆内容"
                            },
                            "confidence": {
                                "type": "number",
                                "description": "本条记忆的可信度。请依据之前的记忆增减。",
                                "minimum": 0.0,
                                "maximum": 1.0
                            }
                        },
                        "required": ["id", "content", "confidence"]
                    }
                }
            },
            "required": ["memories"]
        })
    }

    async fn call(&self, args: Value, _msg: &Message) -> anyhow::Result<Value> {

        let memories = extract!(args, "memories", as_array);
        let length = memories.len();

        for item in memories {
            let id = extract!(item, "id", as_i64) as i32;
            let content = extract!(item, "content", as_str);
            let confidence = extract!(item, "confidence", as_f64);
            self.service.merge(id, &content, confidence).await?;
        }

        get_logger().info(&format!("更新了 {} 条记忆", length));

        Ok(json!({}))
    }
}

pub struct AddMemoryTool {
    pub service: Arc<MemoryService>
}


#[async_trait]
impl Tool for AddMemoryTool {
    fn name(&self) -> &str {
        "add_memory"
    }

    fn description(&self) -> &str {
        "创建一条新的记忆"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "porperties": {
                "content": {
                    "type": "string",
                    "description": "记忆内容"
                }
            },
            "required": ["content"]
        })
    }

    async fn call(&self, args: Value, msg: &Message) -> anyhow::Result<Value> {

        let content = extract!(args, "content", as_str);
        self.service.create(Scope::from(msg), &content).await?;

        Ok(json!({}))
    }
}

pub struct DeleteMemoryTool {
    pub service: Arc<MemoryService>
}

#[async_trait]
impl Tool for DeleteMemoryTool {
    fn name(&self) -> &str {
        "delete_memory"
    }

    fn description(&self) -> &str {
        "删除本条记忆。慎用！"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "porperties": {
                "memory_ids": {
                    "type": "array",
                    "items": {
                        "type": "integer",
                        "description": "要删除的记忆ID"
                    }
                }
            },
            "required": ["memory_ids"]
        })
    }

    async fn call(&self, args: Value, _msg: &Message) -> anyhow::Result<Value> {

        let ids = extract!(args, "ids", as_array);
        let length = ids.len();

        for id in ids {
            if let Some(id) = id.as_i64() {
                self.service.delete(id as i32).await?;
            }
        }

        get_logger().info(&format!("更新了 {} 条记忆", length));
        Ok(json!({}))
    }
}

pub struct SearchMemoryTool {
    pub service: Arc<MemoryService>
}

#[async_trait]
impl Tool for SearchMemoryTool {
    fn name(&self) -> &str {
        "search_memory"
    }

    fn description(&self) -> &str {
        "从记忆库中查找记忆"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keyword": {
                    "type": "string",
                    "description": "要查找的关键词，可以是事件名|用户id|概念等"
                }
            },
            "required": ["keyword"]
        })
    }

    async fn call(&self, args: Value, msg: &Message) -> anyhow::Result<Value> {

        let keyword = extract!(args, "keyword", as_str);
        let similars = self.service.similars(Scope::from(msg), &keyword).await?;
        let result = similars.iter().map(|mem| mem.simplified_plain())
            .collect::<Vec<String>>().join("\n");

        Ok(Value::String(result))
    }
}

pub struct AddAliasTool {
    pub map: Arc<Mutex<AliasesMapping>>
}

#[async_trait]
impl Tool for AddAliasTool {
    fn name(&self) -> &str {
        "add_alias"
    }
    
    fn description(&self) -> &str {
        "添加记录某个用户的某个别称."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "user_id": {
                    "type": "string",
                    "description": "用户id。由纯数字组成"
                },
                "alias": {
                    "type": "string",
                    "description": "要添加的别称"
                }
            },
            "required": ["user_id", "alias"]
        })
    }

    async fn call(&self, args: Value, _msg: &Message) -> anyhow::Result<Value> {

        let user_id = extract!(args, "user_id", as_str).parse::<usize>()?;
        let alias = extract!(args, "alias", as_str);

        self.map.lock().unwrap().insert(user_id, alias.clone());

        Ok(Value::String(format!("添加成功: {} -> {}", alias, user_id)))
    }
}
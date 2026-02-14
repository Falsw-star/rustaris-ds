use std::{collections::HashMap, sync::Arc, time::Duration};

use rust_mc_status::{McClient, ServerEdition};
use serde_json::{Value, json};

use async_trait::async_trait;
use crate::{get_logger, memory::{MemoryService, Scope}, objects::Message};



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

pub struct SaveMemoryTool {
    pub mem_service: Arc<MemoryService>
}

#[async_trait]
impl Tool for SaveMemoryTool {
    fn name(&self) ->  &str {
        "save_memory"
    }
    
    fn description(&self) ->  &str {
        "将信息存入长期记忆"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "本条记忆的大分类。如：user_preference | minecraft_server | event"
                },
                "key": {
                    "type": "string",
                    "description": "用于精确更新或覆盖的唯一键"
                },
                "content": {
                    "type": "string",
                    "description": "自然语言形式的记忆内容"
                },
                "metadata": {
                    "type": "object",
                    "description": "结构化附加信息。以额外键值对表征本条记忆的核心内容。",
                    "properties": {
                        "subject": {
                            "type": "string",
                            "description": "记忆的核心对象"
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "简短分类标签"
                        },
                        "confidence": {
                            "type": "number",
                            "minimum": 0,
                            "maximum": 1,
                            "description": "对该记忆准确性的置信度"
                        }
                    },
                    "required": ["subject", "confidence"],
                    "additionalProperties": true,
                }
            },
            "required": ["category", "key", "content"]
        })
    }

    async fn call(&self, args: Value, msg: &Message) -> anyhow::Result<Value> {
        if let Some(m) = extract_optional!(args, "metadata", as_object) {
            println!("{:?}", m);
        }
        self.mem_service.upsert(
            Scope::from(msg),
            extract!(args, "category", as_str).as_str(),
            extract!(args, "key", as_str).as_str(),
            extract!(args, "content", as_str).as_str(),
            if let Some(metadata) = extract_optional!(args, "metadata", as_object) {
                Some(Value::from(metadata))
            } else { None }
        ).await?;
        Ok(Value::String("保存成功".to_string()))
    }
}

pub struct SearchMemoryTool {
    pub mem_service: Arc<MemoryService>
}

#[async_trait]
impl Tool for SearchMemoryTool {
    fn name(&self) -> &str {
        "search_memory"
    }

    fn description(&self) -> &str {
        "查找记忆信息"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "要搜索的准确唯一的关键词"
                }
            },
            "required": ["query"]
        })
    }

    async fn call(&self, args: Value, msg: &Message) -> anyhow::Result<Value> {
        let query = extract!(args, "query", as_array);
        let query = query.iter().map(
            |key| key.as_str().ok_or_else(|| anyhow::anyhow!("Error parsing key: {}", key))
            .map(|s| s.to_string())
        ).collect::<Result<Vec<_>, _>>()?;
        
        let mut array: Vec<Value> = Vec::new();
        for key in &query {
            let mems = self.mem_service.search(Scope::from(msg), key).await?;
            if mems.len() == 0 {
                array.push(json!({
                    "keyword": key.clone(),
                    "result": "没有找到任何结果"
                }));
            } else {
                array.push(json!({
                    "keyword": key.clone(),
                    "result": mems.iter().map(|mem| {mem.format()}).collect::<Vec<Value>>()
                }));
            }
        }
        Ok(Value::String(Value::Array(array).to_string()))
    }
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
                "is_java": {
                    "type": "boolean",
                    "default": true,
                    "description": "待查服务器是否为 java 端。true则为java端，false则为bedrock端"
                }
            },
            "required": ["address", "is_java"]
        })
    }

    async fn call(&self, args: Value, _msg: &Message) -> anyhow::Result<Value> {
        let address = extract!(args, "address", as_str);
        let is_java = extract!(args, "is_java", as_bool);

        let status = self.client.ping(
            &address.trim(),
            if is_java { ServerEdition::Java }
            else { ServerEdition::Bedrock }
        ).await?;

        Ok(Value::String(serde_json::to_string(&status)?))
    }
}
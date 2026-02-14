use std::{sync::{Arc, Mutex}, time::Duration};
use reqwest::Client;
use serde_json::{Map, Value, json};
use tokio::{select, sync::mpsc, time::sleep};

use crate::{CONFIG, POSTER, adapters::{API, APIError, APIReceiver, APIRequest, APIResponse, APIWrapper}, get_logger, objects::MessageArrayItem};

pub struct PosterNapCat {
    receiver: APIReceiver,
    pub status: Arc<Mutex<bool>>,
    client: Client
}

macro_rules! extract {
    ($map:expr, $key:literal, $extractor:ident) => {
        $map.remove($key)
            .and_then(|v| v.$extractor().map(|o| o.to_owned()))
            .ok_or_else(|| APIError::APIError(format!("Missing field: {}", $key)))?
    };
}

impl PosterNapCat {
    pub fn init(status: Arc<Mutex<bool>>) -> Self {
        let (tx, rx) = mpsc::unbounded_channel::<APIRequest>();
        POSTER.lock().unwrap().replace(APIWrapper { sender: tx });
        Self {
            receiver: rx,
            status: status,
            client: Client::new()
        }
    }

    pub async fn handle(&self, req: APIRequest) {
        match req.api {
            API::SendGroupMsg { group_id, content } => {
                match self.post("send_group_msg", json!({
                    "group_id": group_id,
                    "message":  MessageArrayItem::format_array(content)
                })).await {
                    Ok(res) => {
                        let _ = req.resp_tx.send(APIResponse::from_res(res, |mut map| {
                            Ok(APIResponse::SendMsgResult {
                                success: match extract!(map, "status", as_str).as_str() {
                                    "ok" => true, _ => false
                                },
                                message_id: extract!(extract!(map, "data", as_object), "message_id", as_u64) as usize
                            })
                        }));
                    }
                    Err(err) => {
                        let _ = req.resp_tx.send(err.into());
                    }
                }
            }
            API::SendPrivateMsg { user_id, content } => {
                match self.post("send_private_msg", json!({
                    "user_id": user_id,
                    "message":  MessageArrayItem::format_array(content)
                })).await {
                    Ok(res) => {
                        let _ = req.resp_tx.send(APIResponse::from_res(res, |mut map| {
                            Ok(APIResponse::SendMsgResult {
                                success: match extract!(map, "status", as_str).as_str() {
                                    "ok" => true, _ => false
                                },
                                message_id: extract!(extract!(map, "data", as_object), "message_id", as_u64) as usize
                            })
                        }));
                    }
                    Err(err) => {
                        let _ = req.resp_tx.send(err.into());
                    }
                }
            }
            API::SendGroupText { group_id, content } => {
                match self.post("send_group_msg", json!({
                    "group_id": group_id,
                    "message": content
                })).await {
                    Ok(res) => {
                        let _ = req.resp_tx.send(APIResponse::from_res(res, |mut map| {
                            Ok(APIResponse::SendMsgResult {
                                success: match extract!(map, "status", as_str).as_str() {
                                    "ok" => true, _ => false
                                },
                                message_id: extract!(extract!(map, "data", as_object), "message_id", as_u64) as usize
                            })
                        }));
                    }
                    Err(err) => {
                        let _ = req.resp_tx.send(err.into());
                    }
                }
            }
            API::SendPrivateText { user_id, content } => {
                match self.post("send_private_msg", json!({
                    "user_id": user_id,
                    "message": content
                })).await {
                    Ok(res) => {
                        let _ = req.resp_tx.send(APIResponse::from_res(res, |mut map| {
                            Ok(APIResponse::SendMsgResult {
                                success: match extract!(map, "status", as_str).as_str() {
                                    "ok" => true, _ => false
                                },
                                message_id: extract!(extract!(map, "data", as_object), "message_id", as_u64) as usize
                            })
                        }));
                    }
                    Err(err) => {
                        let _ = req.resp_tx.send(err.into());
                    }
                }
            }
        }
    }

    pub async fn run(&mut self) {
        loop {
            select! {
                Some(req) = self.receiver.recv() => {
                    self.handle(req).await;
                }
                _ = sleep(Duration::from_millis(100)) => {
                    if !*self.status.lock().unwrap() {
                        *POSTER.lock().unwrap() = None;
                        return;
                    }
                }
            }
        }
    }

    async fn post(&self, end: &str, json: Value) -> Result<Map<String, Value>, APIError> {
        let res = self.client
            .post(format!("{}/{}", CONFIG.network.http.trim_matches('/'), end))
            .header("Authorization", format!("Bearer {}", &CONFIG.network.login_token))
            .json(&json)
            .send().await?
            .text().await?;
        
        get_logger().debug(&res);
        let res_body = serde_json::from_str::<Map<String, Value>>(&res)?;
        Ok(res_body)
    }
}


impl Into<APIResponse> for APIError {
    fn into(self) -> APIResponse {
        APIResponse::Error { message: self.to_string() }
    }
}

impl MessageArrayItem {
    fn format(&self) -> Value {
        match self {
            MessageArrayItem::Text(content) => json!({
                "type": "text",
                "data": {
                    "text": content
                }
            }),
            MessageArrayItem::Face(face_id) => json!({
                "type": "face",
                "data": {
                    "id": face_id.to_string()
                }
            }),
            MessageArrayItem::At(user_id) => {
                let qq = match user_id {
                    0 => "all".to_string(),
                    _ => user_id.to_string()
                };
                json!({
                    "type": "at",
                    "data": {
                        "qq": qq
                    }
                })
            },
            MessageArrayItem::Image {
                summary: _,
                file: _,
                url,
                file_size: _
            } => json!({
                "type": "image",
                "data": {
                    "file": url
                }
            })
        }
    }

    pub fn format_array(item_array: Vec<MessageArrayItem>) -> Value {
        Value::Array(item_array.iter().map(|i| i.format()).collect())
    }
}

impl APIResponse {
    pub fn from_res(map: Map<String, Value>, f: fn(Map<String, Value>) -> Result<APIResponse, APIError>) -> APIResponse {
        match (f)(map) {
            Ok(res) => res,
            Err(err) => err.into()
        }
    }
}
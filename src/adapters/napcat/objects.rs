use serde::{Deserialize, Serialize, de::Error};
use serde_json::{Map, Value};

use crate::{objects::{Event, Group, Message, MessageArrayItem, Permission, User}, self_id};

#[derive(Debug, Serialize)]
pub enum MetaEvent {
    Heartbeat {
        online: bool,
        good: bool
    },
    Connected {
        self_id: usize
    }
}

#[derive(Debug)]
pub enum NapCatPost {
    MetaEvent(MetaEvent),
    Event(Event),
    Other
}


macro_rules! extract {
    ($map:expr, $key:literal, $extractor:ident) => {
        $map.remove($key)
            .and_then(|v| v.$extractor().map(|o| o.to_owned()))
            .ok_or_else(|| serde::de::Error::missing_field($key))?
    };
}

macro_rules! extract_optional {
    ($map:expr, $key:literal, $extractor:ident) => {
        $map.remove($key)
            .and_then(|v| v.$extractor().map(|o| o.to_owned()))
    };
}

impl<'a> Deserialize<'a> for NapCatPost {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'a> {
        let mut map = Map::<String, Value>::deserialize(deserializer)?;

        let post_type = extract!(map, "post_type", as_str);

        let post: NapCatPost = match post_type.as_str() {
            "meta_event" => {
                let meta_event_type = extract!(map, "meta_event_type", as_str);
                match meta_event_type.as_str() {
                    "heartbeat" => {
                        let mut status = extract!(map, "status", as_object);
                        let online = extract!(status, "online", as_bool);
                        let good = extract!(status, "good", as_bool);
                        NapCatPost::MetaEvent(MetaEvent::Heartbeat { online, good })
                    }
                    "lifecycle" => NapCatPost::MetaEvent(MetaEvent::Connected {
                        self_id: extract!(map, "self_id", as_u64) as usize
                    }),
                    _ => NapCatPost::Other
                }
            }
            "message" => {
                let message_id = extract!(map, "message_id", as_u64) as usize;

                let mut group: Option<Group> = None;
                let private = match extract!(map, "message_type", as_str).as_str() {
                    "private" => true, "group" => {
                        group = Some(Group {
                            group_id: extract!(map, "group_id", as_u64) as usize,
                            group_name: extract_optional!(map, "group_name", as_str)
                        });
                        false
                    }, _ => false
                };


                let mut sender = extract!(map, "sender", as_object);
                let sender = User {
                    user_id: extract!(sender, "user_id", as_u64) as usize,
                    nickname: extract_optional!(sender, "nickname", as_str),
                    card: extract_optional!(sender, "card", as_str),
                    role: match extract_optional!(sender, "role", as_str) {
                        Some(role) => match role.as_str() {
                            "admin" => Permission::GroupAdmin,
                            _ => Permission::Normal
                        }
                        None => Permission::Normal
                    }
                };

                let raw_message = extract!(map, "raw_message", as_str);
                
                let message_format = extract!(map, "message_format", as_str);

                let message_array = match message_format.as_str() {
                    "array" => {
                        let mut array = Vec::<MessageArrayItem>::new();

                        let original = extract!(map, "message", as_array);
                        for item in original {
                            if let Some(mut item) = item.as_object().map(|o| o.to_owned()) {
                                let item_type = extract!(item, "type", as_str);
                                let mut data = extract!(item, "data", as_object);
                                match item_type.as_str() {
                                    "text" => array.push(MessageArrayItem::Text(extract!(data, "text", as_str))),
                                    "face" => array.push(MessageArrayItem::Face(extract!(data, "id", as_u64) as usize)),
                                    "image" => array.push(MessageArrayItem::Image {
                                        summary: extract_optional!(data, "summary", as_str),
                                        file: extract_optional!(data, "file", as_str),
                                        url: extract!(data, "url", as_str),
                                        file_size: extract_optional!(data, "file_size", as_u64).and_then(|u| Some(u as usize))
                                    }),
                                    "at" => {
                                        let qq = extract!(data, "qq", as_str);
                                        match qq.as_str() {
                                            "all" => array.push(MessageArrayItem::At(self_id())),
                                            _ => array.push(MessageArrayItem::At(qq.parse::<usize>()
                                                .map_err(|e| D::Error::invalid_value(
                                                    serde::de::Unexpected::Str(&e.to_string()),
                                                    &"valid QQ number"
                                                ))?)),
                                        }
                                    },
                                    _ => ()
                                }
                            }
                        }
                        array
                    }
                    "string" => Vec::new(),
                    _ => Vec::new()
                };
                NapCatPost::Event(Event::Message(Message { message_id, private, group, sender, raw: raw_message, array: message_array }))
            }
            _ => NapCatPost::Other
        };
        Ok(post)
    }
}


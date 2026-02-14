use std::{collections::VecDeque};

use serde::{Serialize};

use crate::get_poster;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Clone)]
pub enum Permission {
    Normal,
    GroupAdmin,
    GroupOwner,
    Admin
}

#[derive(Debug, Clone)]
pub struct User {
    pub user_id: usize,
    /// The nickname to the user defined in the bot's qq account.
    pub nickname: Option<String>,
    /// The name user defined in a group. This is an [Option].
    pub card: Option<String>,
    pub role: Permission
}

#[derive(Debug, Clone)]
pub struct Group {
    pub group_id: usize,
    pub group_name: Option<String>
}

#[derive(Debug, Clone)]
pub enum MessageArrayItem {
    Text(String),
    Face(usize),
    Image {
        summary: Option<String>,
        /// The file's name,
        /// like: "13DD8F7493211F96079321FC9B6130CC.jpg"
        file: Option<String>,
        url: String,
        file_size: Option<usize>
    },
    At(usize)
}

#[derive(Debug)]
pub enum Event {
    Message(Message)
}

#[derive(Debug, Clone)]
pub struct Message {
    pub message_id: usize,
    pub private: bool,
    pub group: Option<Group>,
    pub sender: User,
    pub raw: String,
    pub array: Vec<MessageArrayItem>
}

impl Message {

    pub fn on_command(&self, p: &str) -> bool {
        if let Some(cmd) = self.to_cmd_array().pop_front() {
            cmd == p
        } else {
            false
        }
    }

    pub fn starts_with(&self, pat: &str) -> bool {
        self.raw.starts_with(pat)
    }

    pub fn ends_with(&self, pat: &str) -> bool {
        self.raw.ends_with(pat)
    }
    pub fn on_at(&self, user_id: usize) -> bool {
        for item in &self.array {
            if let MessageArrayItem::At(at_user_id) = item {
                if *at_user_id == user_id {
                    return true;
                }
            }
        }
        false
    }

    pub async fn quick_send(&self, content: &str) {
        if self.private {
            let _ = get_poster().send_private_text(self.sender.user_id, content).await;
        } else {
            if let Some(group) = &self.group {
                let _ = get_poster().send_group_text(group.group_id, content).await;
            }
        }
    }

    pub fn to_cmd_array(&self) -> VecDeque<&str> {
        self.raw.split(" ").collect::<VecDeque<&str>>()
    }

    pub fn args(&self) -> VecDeque<&str> {
        let mut arr = self.to_cmd_array();
        arr.pop_front();
        arr
    }

    pub fn joint_args(&self) -> String {
        Vec::from(self.args()).join(" ")
    }

    pub fn simplified_plain(&self) -> String {

        let mut result = String::new();

        for item in &self.array {
            let str_item = match item {
                MessageArrayItem::At(user_id) => format!("@<{}>", user_id),
                MessageArrayItem::Face(_id) => "".to_string(),
                MessageArrayItem::Image {
                    summary,
                    file,
                    url: _,
                    file_size: _
                } => format!("Image<{} {}>", summary.clone().unwrap_or("".to_string()), file.clone().unwrap_or("".to_string())),
                MessageArrayItem::Text(text) => text.clone()
            };

            result += &str_item;
            result += " ";
        }

        result
    }
}
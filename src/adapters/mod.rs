use tokio::sync::{mpsc::error::SendError, oneshot::{self, error::RecvError}};

use crate::objects::{Group, MessageArrayItem, User};

pub mod napcat;

#[allow(async_fn_in_trait)]
pub trait Listener {
    async fn run(&mut self);
}

pub enum API {
    SendGroupMsg {
        group_id: usize,
        content: Vec<MessageArrayItem>
    },
    SendPrivateMsg {
        user_id: usize,
        content: Vec<MessageArrayItem>
    },
    SendGroupText {
        group_id: usize,
        content: String
    },
    SendPrivateText {
        user_id: usize,
        content: String
    }
}

pub enum APIResponse {
    SendMsgResult {
        /// If the message has been sent seccessfully.
        success: bool,
        /// The id of the message sent that can be used for recalling.
        /// Sould be `0` if `success` is `false`
        message_id: usize
    },
    GroupInfo(Group),
    UserInfo(User),
    MemberList(Vec<User>),
    Error {
        message: String
    }
}

pub struct APIRequest {
    pub api: API,
    pub resp_tx: oneshot::Sender<APIResponse>
}

pub type APISender = tokio::sync::mpsc::UnboundedSender<APIRequest>;
pub type APIReceiver = tokio::sync::mpsc::UnboundedReceiver<APIRequest>;

pub enum APIError {
    ChannelSend(String),
    ChannelReceive(String),
    APIError(String),
    RequestFailed,
    MismatchedResponse
}

impl APIError {
    pub fn channel_send(e: SendError<APIRequest>) -> Self {
        Self::ChannelSend(e.to_string())
    }
    pub fn channel_receive(e: RecvError) -> Self {
        Self::ChannelReceive(e.to_string())
    }
}

impl From<reqwest::Error> for APIError {
    fn from(value: reqwest::Error) -> Self {
        APIError::APIError(value.to_string())
    }
}

impl From<serde_json::Error> for APIError {
    fn from(value: serde_json::Error) -> Self {
        APIError::APIError(value.to_string())
    }
}

impl ToString for APIError {
    fn to_string(&self) -> String {
        match self {
            APIError::APIError(err) => err.to_string(),
            APIError::ChannelReceive(err) => err.to_string(),
            APIError::ChannelSend(err) => err.to_string(),
            APIError::MismatchedResponse => "Mismatched Response".to_string(),
            APIError::RequestFailed => "Request Failed".to_string()
        }
    }
}

#[derive(Clone)]
pub struct APIWrapper {
    sender: APISender    
}

impl APIWrapper {
    pub async fn send_group_msg(&self, group_id: usize, content: Vec<MessageArrayItem>) -> Result<usize, APIError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(APIRequest {
            api: API::SendGroupMsg { group_id, content },
            resp_tx: tx
        }).map_err(|e| APIError::channel_send(e))?;
        match rx.await.map_err(|e| APIError::channel_receive(e))? {
            APIResponse::SendMsgResult { success, message_id } => {
                if success { Ok(message_id) }
                else { Err(APIError::RequestFailed) }
            }
            APIResponse::Error { message } => Err(APIError::APIError(message)),
            _ => Err(APIError::MismatchedResponse)
        }
    }

    pub async fn send_private_msg(&self, user_id: usize, content: Vec<MessageArrayItem>) -> Result<usize, APIError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(APIRequest {
            api: API::SendPrivateMsg { user_id, content },
            resp_tx: tx
        }).map_err(|e| APIError::channel_send(e))?;
        match rx.await.map_err(|e| APIError::channel_receive(e))? {
            APIResponse::SendMsgResult { success, message_id } => {
                if success { Ok(message_id) }
                else { Err(APIError::RequestFailed) }
            }
            APIResponse::Error { message } => Err(APIError::APIError(message)),
            _ => Err(APIError::MismatchedResponse)
        }
    }

    pub async fn send_group_text(&self, group_id: usize, content: &str) -> Result<usize, APIError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(APIRequest {
            api: API::SendGroupText { group_id, content: content.to_string() },
            resp_tx: tx
        }).map_err(|e| APIError::channel_send(e))?;
        match rx.await.map_err(|e| APIError::channel_receive(e))? {
            APIResponse::SendMsgResult { success, message_id } => {
                if success { Ok(message_id) }
                else { Err(APIError::RequestFailed) }
            }
            APIResponse::Error { message } => Err(APIError::APIError(message)),
            _ => Err(APIError::MismatchedResponse)
        }
    }

    pub async fn send_private_text(&self, user_id: usize, content: &str) -> Result<usize, APIError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(APIRequest {
            api: API::SendPrivateText { user_id, content: content.to_string() },
            resp_tx: tx
        }).map_err(|e| APIError::channel_send(e))?;
        match rx.await.map_err(|e| APIError::channel_receive(e))? {
            APIResponse::SendMsgResult { success, message_id } => {
                if success { Ok(message_id) }
                else { Err(APIError::RequestFailed) }
            }
            APIResponse::Error { message } => Err(APIError::APIError(message)),
            _ => Err(APIError::MismatchedResponse)
        }
    }
}
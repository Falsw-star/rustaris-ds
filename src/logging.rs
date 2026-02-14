use chrono::Local;
use colored::{Color, Colorize};
use tokio::{sync::mpsc::{self, UnboundedReceiver, UnboundedSender}, task::JoinHandle};
use dyn_fmt::AsStrFormatExt;

use crate::{CONFIG, LOGGER};

const META_TEMP: &'static str = "[{}] {} {} {} ";

pub enum LogMsg {
    INFO(String),
    WARN(String),
    ERROR(String),
    CHAT(String),
    DEBUG(String)
}

impl LogMsg {

    pub fn enabled(&self) -> bool {
        match self {
            Self::INFO(_) => CONFIG.logger.info,
            Self::WARN(_) => CONFIG.logger.warning,
            Self::ERROR(_) => CONFIG.logger.error,
            Self::CHAT(_) => CONFIG.logger.chat,
            Self::DEBUG(_) => CONFIG.logger.debug
        }
    }

    pub fn split(&self) -> (&str, &str, Color, &str) {
        match self {
            Self::INFO (content) => ("➡️", "Info ", Color::BrightCyan, content),
            Self::WARN (content) => ("⚠️", "Warn ", Color::Yellow, content),
            Self::ERROR(content) => ("❌", "Error", Color::Red, content),
            Self::CHAT (content) => ("💬", "Chat ", Color::Green, content),
            Self::DEBUG(content) => ("⚙️", "Debug", Color::Magenta, content)
        }
    }
}

pub struct LoggerProvider {
    receiver: UnboundedReceiver<LogMsg>,
}
impl LoggerProvider {
    pub fn init() -> JoinHandle<()> {
        let (sender, receiver) = mpsc::unbounded_channel::<LogMsg>();
        let mut provider = Self { receiver };
        let logger = Logger { sender };
        LOGGER.lock().unwrap().replace(logger);
        tokio::spawn(async move {
            provider.run().await
        })
    }

    pub async fn run(&mut self) {
        loop {
            if let Some(msg) = self.receiver.recv().await {

                if !msg.enabled() {
                    continue;
                }

                let (level_icon, level_str, level_color, content) = msg.split();

                let time = Local::now().format("%H:%M:%S").to_string();
                let meta_len = META_TEMP.format(&[&time, level_icon, level_str, "|"]).len();

                let content = content.replace("\n", &("\n".to_string() + &" ".repeat(meta_len)));

                let time = time.color(Color::BrightBlack).to_string();
                let level_str = level_str.bold().color(level_color).to_string();

                println!("{}", META_TEMP.format(&[&time, level_icon, &level_str, "|"]) + &content);

            } else {
                // If None is returned, that means the original `Logger`
                // in the lazy_lock and all other `Logger`s has been dropped.
                break;
            }
        }
    }

    pub fn exit() {
        *LOGGER.lock().unwrap() = None;
    }
}

#[derive(Clone)]
pub struct Logger {
    sender: UnboundedSender<LogMsg>
}
impl Logger {
    pub fn info(&self, msg: &str) {
        let _ = self.sender.send(LogMsg::INFO(msg.to_string()));
    }

    pub fn warn(&self, msg: &str) {
        let _ = self.sender.send(LogMsg::WARN(msg.to_string()));
    }

    pub fn error(&self, msg: &str) {
        let _ = self.sender.send(LogMsg::ERROR(msg.to_string()));
    }
    
    pub fn chat(&self, msg: &str) {
        let _ = self.sender.send(LogMsg::CHAT(msg.to_string()));
    }

    pub fn debug(&self, msg: &str) {
        let _ = self.sender.send(LogMsg::DEBUG(msg.to_string()));
    }
}
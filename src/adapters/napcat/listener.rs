use std::{collections::VecDeque, sync::{Arc, Mutex}, time::Duration};

use tokio::{select, time::sleep};
use websockets::{Frame, WebSocket, WebSocketError};

use crate::{CONFIG, adapters::Listener, SELFID, adapters::napcat::objects::{MetaEvent, NapCatPost}, get_logger, objects::Event};


pub struct ListenerNapCat {
    pub events: Arc<Mutex<VecDeque<Event>>>,
    pub status: Arc<Mutex<bool>>
}


impl Listener for ListenerNapCat {
    async fn run(&mut self) {
        let logger = get_logger();
        
        while *self.status.lock().unwrap() {
            match self.connect_websocket().await {
                Ok(_) => {},
                Err(e) => {
                    logger.info(&format!("WebSocket connection failed: {}", e));
                    if *self.status.lock().unwrap() {
                        sleep(Duration::from_secs(3)).await;
                        logger.info("Trying to reconnect...");
                    }
                }
            }
        }
    }
}

impl ListenerNapCat {

    pub fn init(status: Arc<Mutex<bool>>) -> Self {
        Self { events: Arc::new(Mutex::new(VecDeque::new())), status }
    }

    async fn connect_websocket(&mut self) -> Result<(), WebSocketError> {
        let mut ws = WebSocket::builder()
            .add_header("Authorization", &format!("Bearer {}", &CONFIG.network.login_token))
            .connect(&CONFIG.network.websocket)
            .await?;
                
        while *self.status.lock().unwrap() {
            select! {
                result = ws.receive() => {
                    self.handle_websocket_frame(result?);
                }
                _ = sleep(Duration::from_millis(100)) => {
                    if !*self.status.lock().unwrap() {
                        let _ = ws.close(None);
                        return Ok(());
                    }
                }
            }
        }
        Ok(())
    }
    
    fn handle_websocket_frame(&mut self, frame: Frame) {
        let logger = get_logger();
        match frame {
            Frame::Text { payload, .. } => {
                match serde_json::from_str::<NapCatPost>(&payload) {
                    Ok(NapCatPost::MetaEvent(meta_event)) => {
                        self.handle_meta_event(meta_event);
                    },
                    Ok(NapCatPost::Event(event)) => {
                        self.events.lock().unwrap().push_back(event);
                    },
                    Ok(NapCatPost::Other) => {},
                    Err(err) => logger.info(&err.to_string()),
                }
            },
            Frame::Close { payload } => {
                let (code, msg) = payload.unwrap_or((0u16, "Unknown".to_string()));
                logger.info(&format!("WebSocket closed: {} - {}", code, msg));
            },
            _ => {}
        }
    }
    
    fn handle_meta_event(&self, meta_event: MetaEvent) {
        let logger = get_logger();
        match meta_event {
            MetaEvent::Heartbeat { online, good } => {
                if !online { logger.info("[Heartbeat] Bot is not online."); }
                if !good { logger.info("[Heartbeat] Bot is not good."); }
            },
            MetaEvent::Connected { self_id } => {
                logger.info(&format!("Bot Connected: {}", self_id));
                SELFID.lock().unwrap().replace(self_id);
            }
        }
    }
}
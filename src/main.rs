use std::{sync::{Arc, Mutex}, time::Duration};

use rustaris_ds::{
    CONFIG, DEV, adapters, commands::run_cmds, get_logger, logging::LoggerProvider, objects::Event, set_exit_handler, thinking::{self, Thinker}
};

use tokio::time::sleep;

#[tokio::main]
async fn main() -> anyhow::Result<()> {

    let logger_thread = LoggerProvider::init();
    let logger = get_logger();

    if DEV { logger.warn("Running in Dev mode..."); }
    dotenv::dotenv().ok();

    let status = Arc::new(Mutex::new(true));
    set_exit_handler(&status);

    let (listener, poster) = adapters::napcat::get_pair();
    let adapter_status = listener.status.clone();
    let events = listener.events.clone();
    let adapter_thread = adapters::napcat::run_pair(listener, poster);

    let thinker = Thinker::init().await?;
    let thinker_status = thinker.status.clone();
    let (thinker_thread, think_end) = thinking::run(thinker);

    while *status.lock().unwrap() {
        if let Some(event) = events.lock().unwrap().pop_front() {
            match event {
                Event::Message(msg) => {
                    logger.chat(&format!("Msg: {} from {}", msg.raw, msg.sender.user_id));
                    if !run_cmds(msg.clone()).await {
                        let _ = think_end.send(msg);
                    }
                }
            }
        }
        sleep(Duration::from_secs_f32(CONFIG.heart_beat)).await;
    }

    logger.info("Exiting......");
    
    *adapter_status.lock().unwrap() = false;
    *thinker_status.lock().unwrap() = false;

    adapter_thread.await?;
    thinker_thread.await?;

    drop(logger);
    LoggerProvider::exit();
    logger_thread.await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_mc_status::McClient;
    use rustaris_ds::memory::{MemoryService, Scope};
    use tokio;

    #[tokio::test]
    async fn test_mem_service() -> anyhow::Result<()> {

        let logger_thread = LoggerProvider::init();

        let mems = MemoryService::init().await?;
        mems.upsert(Scope::Global, "test", "test_key", "test_content", None).await?;
        let result = mems.get(Scope::Global, "test_key").await?.unwrap();
        println!("{}", result.content);

        LoggerProvider::exit();
        logger_thread.await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_mcs() -> anyhow::Result<()> {
        let client = McClient::new().with_max_parallel(5).with_timeout(Duration::from_secs(5));
        let status = client.ping("alive.falsw.top", rust_mc_status::ServerEdition::Java).await?;

        println!("{}", serde_json::to_string(&status)?);
        Ok(())
    }
}
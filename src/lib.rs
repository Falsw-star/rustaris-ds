use std::sync::{Arc, LazyLock, Mutex};

use lazy_static::lazy_static;
use crate::{adapters::APIWrapper, config::Config, logging::Logger};

pub mod config;
pub mod logging;
pub mod adapters;
pub mod objects;
pub mod commands;
pub mod thinking;
pub mod memory;
pub mod tools;


pub const DEV: bool = true;


pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    Config::init()
});

lazy_static! {
    pub static ref LOGGER: Arc<Mutex<Option<Logger>>> =
        Arc::new(Mutex::new(None));
}

pub fn get_logger() -> Logger {
    LOGGER.lock().unwrap().as_ref().cloned().expect("Logger is not initialized")
}

lazy_static! {
    pub static ref SELFID: Arc<Mutex<Option<usize>>> =
        Arc::new(Mutex::new(None));
}

pub fn self_id() -> usize {
    SELFID.lock().unwrap().as_ref().cloned().expect("self_id is not assigned")
}

lazy_static! {
    pub static ref POSTER: Arc<Mutex<Option<APIWrapper>>> =
        Arc::new(Mutex::new(None));
}

pub fn get_poster() -> APIWrapper {
    POSTER.lock().unwrap().as_ref().cloned().expect("Poster is not initialized")
}


pub fn set_exit_handler(status: &Arc<Mutex<bool>>) {
    let exit = status.clone();
    ctrlc::set_handler(move || {
        *exit.lock().unwrap() = false;
    }).expect("Fail to set ctrlc handler");
}
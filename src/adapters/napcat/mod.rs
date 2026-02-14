use std::{sync::{Arc, Mutex}};
use tokio::{spawn, task::JoinHandle};

use crate::adapters::{Listener, napcat::{listener::ListenerNapCat, poster::PosterNapCat}};

pub mod poster;
pub mod listener;
pub mod objects;

pub fn get_pair() -> (ListenerNapCat, PosterNapCat) {
    let status = Arc::new(Mutex::new(true));
    (ListenerNapCat::init(status.clone()), PosterNapCat::init(status.clone()))
}

pub fn run_pair(mut lis: ListenerNapCat, mut pos: PosterNapCat) -> JoinHandle<()> {
    spawn(async move {
        let lis_handle = spawn(async move {
            lis.run().await
        });
        let pos_handle = spawn(async move {
            pos.run().await
        });
        lis_handle.await.unwrap();
        pos_handle.await.unwrap();
    })
}
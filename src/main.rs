#![feature(async_closure)]
#![allow(dead_code)]
mod buffer_pool;
mod kcp;
mod udp;
mod user_client;

use crate::kcp::{KcpConfig, KcpListener, KcpNoDelayConfig, KcpPeer};
use std::error::Error;

use bytes::Buf;
use env_logger::Builder;
use mimalloc::MiMalloc;
use std::sync::Arc;

use lazy_static::lazy_static;
use log::LevelFilter;
use user_client::*;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

lazy_static! {
    pub static ref USER_PEER_MANAGER: Arc<UserClientManager> = UserClientManager::new();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    Builder::new().filter(None, LevelFilter::Info).init();

    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp = KcpListener::<Arc<ClientPeer>, _>::new("0.0.0.0:5555", config).await?;

    kcp.set_kcpdrop_event_input(move |conv| {
        let mut handle = USER_PEER_MANAGER.get_handle();
        handle.remove_peer(conv).unwrap();
    })
    .await;

    kcp.set_buff_input(async move |kcp_peer, mut data| {

        let peer= {
            let mut token = kcp_peer.token.borrow_mut();
            match token.get() {
                None => {
                    if data.len() >= 5 && data.get_u32_le() == 1 && data[4] == 0 {
                        data.advance(1);
                        let mut handle = USER_PEER_MANAGER.get_handle();
                        let peer = Arc::new(ClientPeer::new(
                            kcp_peer.conv,
                            Arc::downgrade(&kcp_peer),
                        ));
                        handle.create_peer(peer.clone())?;
                        token.set(Some(peer.clone()));
                        peer
                    } else {
                        return Ok(());
                    }
                },
                Some(peer) => {
                    peer.clone()
                }
            }
        };

        peer.input_buff(&data).await?;
        Ok(())
    });

    kcp.start().await?;
    Ok(())
}

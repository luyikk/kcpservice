#![feature(async_closure)]
#![allow(dead_code)]
mod buffer_pool;
mod kcp;
mod services;
mod udp;
mod user_client;

use crate::kcp::{KcpConfig, KcpListener, KcpNoDelayConfig, KcpPeer};

use services::ServicesManager;
use user_client::*;

use std::error::Error;
use std::sync::Arc;

use bytes::Buf;
use env_logger::Builder;
use mimalloc::MiMalloc;
use json::JsonValue;
use lazy_static::lazy_static;
use log::LevelFilter;


#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

lazy_static! {

    /// 配置文件
    pub static ref SERVICE_CFG:JsonValue={
        if let Ok(json)= std::fs::read_to_string("./service_cfg.json") {
            json::parse(&json).unwrap()
        }
        else{
            panic!("not found service_cfg.json");
        }
    };

     /// 用户管理
    pub static ref USER_PEER_MANAGER: Arc<UserClientManager> = UserClientManager::new();

     /// 服务管理
    pub static ref SERVICE_MANAGER:Arc<ServicesManager>=ServicesManager::new(&SERVICE_CFG,USER_PEER_MANAGER.get_handle()).unwrap();


}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    Builder::new().filter(None, LevelFilter::Debug).init();

    SERVICE_MANAGER.start().await?;


    let timeout_second = SERVICE_CFG["clientTimeoutSeconds"].as_i64().unwrap();
    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp =
        KcpListener::<Arc<ClientPeer>, _>::new("0.0.0.0:5555", config, timeout_second).await?;

    kcp.set_kcpdrop_event_input(move |conv| {
        let mut handle = USER_PEER_MANAGER.get_handle();
        handle.remove_peer(conv).unwrap();
    })
    .await;

    kcp.set_buff_input(async move |kcp_peer, mut data| {
        let peer = {
            let mut token = kcp_peer.token.borrow_mut();
            match token.get() {
                None => {
                    if data.len() >= 5 && data.get_u32_le() == 1 && data[4] == 0 {
                        data.advance(1);
                        let mut handle = USER_PEER_MANAGER.get_handle();
                        let peer =
                            Arc::new(ClientPeer::new(kcp_peer.conv, Arc::downgrade(&kcp_peer)));
                        handle.create_peer(peer.clone())?;
                        token.set(Some(peer.clone()));
                        peer
                    } else {
                        return Ok(());
                    }
                }
                Some(peer) => peer.clone(),
            }
        };

        peer.input_buff(&data).await?;
        Ok(())
    });

    kcp.start().await?;
    Ok(())
}

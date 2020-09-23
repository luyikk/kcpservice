#![feature(async_closure)]
mod udp;
mod kcp;
mod client;
mod buffer_pool;

use std::error::Error;
use crate::kcp::{KcpConfig, KcpNoDelayConfig, KcpListener,KcpPeer};

use mimalloc::MiMalloc;
use std::sync::Arc;
use bytes::{Bytes, Buf};
use env_logger::Builder;
use log::LevelFilter;
use client::ClientPeer;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main()->Result<(),Box<dyn Error>> {
    Builder::new()
        .filter(None, LevelFilter::Info)
        .init();

    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp = KcpListener::<Arc<ClientPeer>, _>::new("0.0.0.0:5555", config).await?;
    kcp.set_buff_input(buff_input).await;
    kcp.start().await?;
    Ok(())
}

async fn buff_input(kcp_peer:Arc<KcpPeer<Arc<ClientPeer>>>, mut data:Bytes) ->Result<(),Box<dyn Error>> {

    let  peer= {
        let mut client_peer = kcp_peer.token.borrow_mut();
        match client_peer.get() {
            None => {
                if data.len()>=5 &&
                    data.get_u32_le()==1 &&
                    data[4] == 0 {
                    data.advance(1);
                    let cp = Arc::new(ClientPeer::new(kcp_peer.conv,Arc::downgrade(&kcp_peer)));
                    client_peer.set(Some(cp.clone()));
                    cp.open(0);
                    cp
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
}

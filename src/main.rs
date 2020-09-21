#![feature(async_closure)]
mod udp;
mod kcp;

use std::error::Error;
use crate::kcp::{KcpConfig, KcpNoDelayConfig, KcpListener,KcpPeer};

use mimalloc::MiMalloc;
use std::sync::Arc;
use bytes::Bytes;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[tokio::main]
async fn main()->Result<(),Box<dyn Error>> {
    let mut config = KcpConfig::default();
    config.nodelay = Some(KcpNoDelayConfig::fastest());
    let kcp = KcpListener::<i32, _>::new("0.0.0.0:5555", config).await?;
    kcp.set_buff_input(buff_input).await;
    kcp.start().await?;
    Ok(())
}

async fn buff_input(peer:Arc<KcpPeer<i32>>,data:Bytes)->Result<(),Box<dyn Error>>{
    peer.send(&data).await?;
    Ok(())
}

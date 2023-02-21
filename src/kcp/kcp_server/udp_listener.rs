use crate::udp::SendUDP;
use crate::udp::{RevType, UdpServer};
use anyhow::*;
use async_trait::*;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

/// 为了封装UDP server 去掉无关的泛型参数
/// 定义了一个trait
#[async_trait]
pub trait UdpListener: Send + Sync {
    fn get_msg_tx(&self) -> Option<Sender<RevType>>;
    async fn start(&self) -> Result<()>;
}

#[async_trait]
impl<I, R, S> UdpListener for UdpServer<I, R, S>
where
    I: Fn(Arc<S>, SendUDP, SocketAddr, Vec<u8>) -> R + Send + Sync + 'static,
    R: Future<Output = Result<()>> + Send,
    S: Send + Sync + 'static,
{
    fn get_msg_tx(&self) -> Option<Sender<RevType>> {
        self.get_msg_tx()
    }

    /// 实现 UDPListener的 start
    async fn start(&self) -> Result<()> {
        self.start().await?;
        Ok(())
    }
}

use std::error::Error;
use crate::udp::UdpServer;
use std::sync:: Arc;
use std::future::Future;
use futures::executor::block_on;
use std::net::SocketAddr;
use tokio::sync::Mutex;
use tokio::net::udp::SendHalf;

/// 为了封装UDP server 去掉无关的泛型参数
/// 定义了一个trait
pub trait UdpListener: Send + Sync {
    fn start(&self) -> Result<(), Box<dyn Error>>;
}

impl<I, R, S> UdpListener for UdpServer<I, R, S>
    where
        I: Fn(Arc<S>, Arc<Mutex<SendHalf>>, SocketAddr, Vec<u8>) -> R + Send + Sync + 'static,
        R: Future<Output = Result<(), Box<dyn Error>>> + Send,
        S: Send + Sync + 'static,
{
    /// 实现 UDPListener的 start
    fn start(&self) -> Result<(), Box<dyn Error>> {
        block_on(async move { self.start().await })
    }
}





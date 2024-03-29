use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

pub type SendUDP = UnboundedSender<(Vec<u8>, SocketAddr)>;

pub struct SendPool {
    mpsc_sender: SendUDP,
}

unsafe impl Send for SendPool {}
unsafe impl Sync for SendPool {}

impl SendPool {
    pub fn new(udp_send: Arc<UdpSocket>) -> SendPool {
        let (tx, rx) = unbounded_channel();
        Self::recv(rx, udp_send);
        SendPool { mpsc_sender: tx }
    }

    pub fn get_tx(&self) -> SendUDP {
        self.mpsc_sender.clone()
    }

    fn recv(mut mpsc_receiver: UnboundedReceiver<(Vec<u8>, SocketAddr)>, udp_send: Arc<UdpSocket>) {
        tokio::spawn(async move {
            while let Some((data, addr)) = mpsc_receiver.recv().await {
                let _ = udp_send.send_to(&data, &addr).await;
            }
        });
    }
}

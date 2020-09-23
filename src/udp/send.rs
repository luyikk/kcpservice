use tokio::net::udp::SendHalf;
use tokio::sync::mpsc::{channel,Receiver,Sender};
use std::option::Option::Some;
use std::net::SocketAddr;

pub type SendUDP=Sender<(Vec<u8>,SocketAddr)>;

pub struct SendPool{
    mpsc_sender:SendUDP
}

unsafe impl Send for SendPool{}
unsafe impl Sync for SendPool{}

impl SendPool{
    pub fn new(udp_send:SendHalf)->SendPool{
        let (tx,rx)=channel(1024);
        Self::recv(rx,udp_send);
        SendPool{
            mpsc_sender:tx,
        }
    }

    pub fn get_tx(&self)->SendUDP{
        self.mpsc_sender.clone()
    }

    fn recv(mut mpsc_receiver: Receiver<(Vec<u8>,SocketAddr)>, mut udp_send:SendHalf){
        tokio::spawn(async move{
            while let Some((data,addr))=mpsc_receiver.recv().await{
               udp_send.send_to(&data,&addr).await;
            }
        });
    }

}
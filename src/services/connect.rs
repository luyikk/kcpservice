use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{channel, Receiver, UnboundedSender, unbounded_channel, UnboundedReceiver};
use log::*;
use tokio::sync::mpsc::error::SendError;
use super::ServicesCmd;
use std::io;
use std::result::Result::Ok;
use xbinary::XBWrite;


pub struct Connect{
    pub read_rx:Option<Receiver<Vec<u8>>>
}

impl Connect{
    pub async fn new(addr:String,disconnect:Box<dyn FnOnce()->Result<(), SendError<ServicesCmd>>+'static+Send>)->io::Result<(Connect,UnboundedSender<XBWrite>)>{
        let (mut reader, mut writer)=TcpStream::connect(&addr).await?.into_split();
        let (mut tx,rx)=channel(1024);

        tokio::spawn(async move{
             while let Ok(len)=  reader.read_u32_le().await {
                 let mut buff=vec![0;len as usize];
                 if let Err(_)=reader.read(&mut buff).await{
                     break
                 }
                 if let Err(er)= tx.send(buff).await{
                     error!{"recv data send error:{}",er}
                 }
             }
            debug!("disconnect to {}",addr);
             if let Err(er)=  disconnect() {
                 error!{"disconnect error:{}",er}
             }
        });

        let (tx,mut rx_send):(UnboundedSender<XBWrite>,UnboundedReceiver<XBWrite>)=unbounded_channel();
        tokio::spawn(async move{
            while let Some(ref data)=rx_send.recv().await{
                 if  writer.write(data).await.is_err(){
                    break;
                 }
            }
            debug!("tcp send drop");
        });

        Ok((Connect{
            read_rx:Some(rx)
        },tx))
    }


}
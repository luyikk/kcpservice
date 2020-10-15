use super::ServicesCmd;
use log::*;
use std::io;
use std::result::Result::Ok;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender,
};
use xbinary::XBWrite;

#[derive(Debug)]
pub enum ConnectCmd {
    Buff(Vec<u8>),
    DropClient(u32),
}

pub struct Connect {
    pub read_rx: Option<Receiver<ConnectCmd>>,
    pub read_tx: Sender<ConnectCmd>,
}

impl Connect {
    pub async fn new(
        addr: String,
        disconnect: Box<dyn FnOnce() -> Result<(), SendError<ServicesCmd>> + 'static + Send>,
    ) -> io::Result<(Connect, UnboundedSender<XBWrite>)> {
        let (mut reader, mut writer) = TcpStream::connect(&addr).await?.into_split();
        let (mut tx, rx) = channel(1024);
        let read_tx = tx.clone();
        let s_addr = addr.clone();
        tokio::spawn(async move {
            while let Ok(len) = reader.read_u32_le().await {
                let mut buff = vec![0; len as usize];
                if reader.read_exact(&mut buff).await.is_err() {
                    break;
                }
                if let Err(er) = tx.send(ConnectCmd::Buff(buff)).await {
                    error! {"service:{} recv data send error:{}",addr,er}
                    break;
                }
            }
            debug!("disconnect to {}", addr);
            if let Err(er) = disconnect() {
                error! {"disconnect error:{}",er}
            }
        });

        let (tx, mut rx_send): (UnboundedSender<XBWrite>, UnboundedReceiver<XBWrite>) =
            unbounded_channel();

        tokio::spawn(async move {
            while let Some(ref data) = rx_send.recv().await {
                if !data.is_empty() {
                    if writer.write(data).await.is_err() {
                        break;
                    }
                } else {
                    debug!("shutdown tcp connect:{}", s_addr);
                    if let Err(er) = writer.shutdown().await {
                        error!("shutdown tcp client error:{}->{:?}", er, er);
                    }
                }
            }
            debug!("tcp send drop");
        });

        Ok((
            Connect {
                read_rx: Some(rx),
                read_tx,
            },
            tx,
        ))
    }

    ///获取读取句柄
    pub fn get_read_tx(&self) -> Sender<ConnectCmd> {
        self.read_tx.clone()
    }
}

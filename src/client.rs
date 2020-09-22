use std::sync::{Weak, Arc};
use crate::kcp::KcpPeer;
use crate::buffer_pool::BuffPool;
use bytes::{Bytes, Buf, BufMut};
use std::error::Error;
use log::*;
use std::net::SocketAddr;
use xbinary::*;
use std::cell::RefCell;
use tokio::sync::Mutex;

/// 玩家PEER
pub struct ClientPeer{
    pub session_id:u32,
    pub kcp_peer:Weak<KcpPeer<Arc<ClientPeer>>>,
    pub buff_pool:Mutex<BuffPool>
}

impl Drop for ClientPeer{
    fn drop(&mut self) {
        info!("client_peer:{} is drop",self.session_id);
    }
}

impl ClientPeer{
    /// 创建一个玩家PEER
    pub fn new(session_id:u32, kcp_peer:Weak<KcpPeer<Arc<ClientPeer>>>)->ClientPeer{
        ClientPeer{
            session_id,
            kcp_peer,
            buff_pool:Mutex::new(BuffPool::new(512*1024))
        }
    }

    /// 获取IP地址+端口号
    pub fn get_addr(&self)->Option<SocketAddr>{
        self.kcp_peer.upgrade().map(|x|{
            x.addr
        })
    }

    /// OPEN 服务器
    pub fn open(&self,_server_id:u32){

    }

    /// 网络数据包输入,处理
    pub async fn input_buff(&self,buff:&[u8])->Result<(),Box<dyn Error>>{
        let mut buff_pool=self.buff_pool.lock().await;

        buff_pool.write(buff);
        loop{
            match buff_pool.read() {
                Ok(data)=>{
                    if let Some(data) = data {
                        self.input_data(Bytes::from(data)).await?;
                    } else {
                        buff_pool.advance();
                        break;
                    }
                },
                Err(msg)=>{
                    error!("{}-{:?} error:{}",self.session_id,self.get_addr(),msg);
                    buff_pool.reset();
                    break;
                }
            }
        }

        Ok(())
    }

    /// 数据包处理
    async fn input_data(&self,data:Bytes)->Result<(),Box<dyn Error>>{
        let mut reader=XBRead::new(data);
        let server_id= reader.get_u32_le();
        match server_id {
            0xFFFFFFFF=>{

                self.send(server_id,&reader).await;

            },
            _=>{

            }
        }

        Ok(())
    }

    pub async fn send(&self,session_id:u32,data:&[u8])->Result<usize,Box<dyn Error>>{
        if let Some(kcp_peer)= self.kcp_peer.upgrade() {
            let mut writer=XBWrite::new();
            writer.put_u32_le(0);
            writer.put_u32_le(session_id);
            writer.write(data);
            writer.set_position(0);
            writer.put_u32_le(writer.len() as u32 -4);
            return Ok(kcp_peer.send(&writer).await?)
        }
        Ok(0)
    }

}
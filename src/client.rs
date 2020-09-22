use std::sync::{Weak, Arc};
use crate::kcp::KcpPeer;
use crate::buffer_pool::BuffPool;
use bytes::Bytes;
use std::error::Error;
use log::*;
use std::net::SocketAddr;

/// 玩家PEER
pub struct ClientPeer{
    pub session_id:u32,
    pub kcp_peer:Weak<KcpPeer<Arc<ClientPeer>>>,
    pub buff_pool:BuffPool
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
            buff_pool:BuffPool::new(512*1024)
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
        self.buff_pool.write(buff).await;
        loop{
            match self.buff_pool.read().await {
                Ok(data)=>{
                    if let Some(data) = data {
                        self.input_data(Bytes::from(data))?;
                    } else {
                        self.buff_pool.advance().await;
                        break;
                    }
                },
                Err(msg)=>{
                    error!("{}-{:?} error:{}",self.session_id,self.get_addr(),msg);
                    self.buff_pool.reset().await;
                    break;
                }
            }
        }

        Ok(())
    }

    /// 数据包处理
    fn input_data(&self,data:Bytes)->Result<(),Box<dyn Error>>{

        Ok(())
    }

    

}
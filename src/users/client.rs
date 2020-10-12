use crate::KcpPeer;
use bytes::{Buf, BufMut, Bytes};
use log::*;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use xbinary::*;

use super::super::services::ServiceHandler;
use super::super::buffer_pool::BuffPool;
use std::cell::RefCell;
use tokio::time::{delay_for, Duration};
use std::sync::atomic::{AtomicBool, Ordering};


/// 玩家PEER
pub struct ClientPeer {
    pub session_id: u32,
    pub kcp_peer: Weak<KcpPeer<Arc<ClientPeer>>>,
    pub buff_pool: RefCell<BuffPool>,
    pub is_open_zero: AtomicBool,
    pub service_handler:ServiceHandler

}

unsafe impl Send for ClientPeer {}
unsafe impl Sync for ClientPeer {}

impl Drop for ClientPeer {
    fn drop(&mut self) {
        info!("client_peer:{} is drop", self.session_id);
    }
}

impl ClientPeer {
    /// 创建一个玩家PEER
    pub fn new(session_id: u32, kcp_peer: Weak<KcpPeer<Arc<ClientPeer>>>,service_handler:ServiceHandler) -> ClientPeer {
        ClientPeer {
            session_id,
            kcp_peer,
            buff_pool: RefCell::new(BuffPool::new(512 * 1024)),
            is_open_zero: AtomicBool::new(false),
            service_handler
        }
    }

    /// 获取IP地址+端口号
    pub fn get_addr(&self) -> Option<SocketAddr> {
        self.kcp_peer.upgrade().map(|x| x.addr)
    }

    /// 向0号服务器发起OPEN
    pub fn open(&self, service_id: u32)-> Result<(), Box<dyn Error>> {
        if let Some(addr)=self.get_addr() {
            self.service_handler.clone().open(self.session_id, service_id, addr.to_string())?;
            debug!("start open service:{} peer:{}",service_id,self.session_id);
        }
        else{
            error!("not found addr by {}",self.session_id);
        }
        Ok(())
    }

    /// 服务器通知 设置OPEN成功
    pub async fn open_service(&self,service_id: u32)-> Result<(), Box<dyn Error>>{
        debug!("service:{} open peer:{} OK",service_id,self.session_id);
        self.is_open_zero.store(true,Ordering::Release);
        self.send_open(service_id).await?;
        Ok(())
    }

    /// 服务器通知 关闭某个服务
    pub fn close_service(&self,service_id: u32){
        if service_id ==0 {

        }
    }

    /// 网络数据包输入,处理
    pub async fn input_buff(&self, buff: &[u8]) -> Result<(), Box<dyn Error>> {
        let input_data_array = {
            let mut input_data_vec = Vec::with_capacity(1);
            let mut buff_pool = self.buff_pool.borrow_mut();
            buff_pool.write(buff);
            loop {
                match buff_pool.read() {
                    Ok(data) => {
                        if let Some(data) = data {
                            input_data_vec.push(Bytes::from(data));
                        } else {
                            break;
                        }
                    }
                    Err(msg) => {
                        error!("{}-{:?} error:{}", self.session_id, self.get_addr(), msg);
                        buff_pool.reset();
                        break;
                    }
                }
            }
            input_data_vec
        };

        for data in input_data_array {
            self.input_data(data).await?;
        }

        Ok(())
    }

    /// 数据包处理
    async fn input_data(&self, data: Bytes) -> Result<(), Box<dyn Error>> {
        let mut reader = XBRead::new(data);
        let server_id = reader.get_u32_le();
        match server_id {
            //给网关发送数据包,默认当PING包无脑回
            0xFFFFFFFF => {
                self.send(server_id, &reader).await?;
            }
            // 指定发送给服务器
            _ => {
                //前置检测 如果没有OPEN 0 不能发送
                if !self.is_open_zero.load(Ordering::Acquire)  {
                    self.kick().await?;
                    info!(
                        "Peer:{}-{:?} not open 0 read data Disconnect it",
                        self.session_id,
                        self.get_addr()
                    );
                    return Ok(());
                }

                self.service_handler.clone().send_buffer(self.session_id, server_id,reader)?;
            }
        }

        Ok(())
    }



    /// 先发送断线包等待多少毫秒清理内存
    pub async fn kick_wait_ms(&self, ms: i32) -> Result<(), Box<dyn Error>> {
        if ms == 3111 {
            self.disconnect_now();
        } else {
            self.send_close(0).await?;
            delay_for(Duration::from_millis(ms as u64)).await;
            self.disconnect_now();
        }
        Ok(())
    }

    /// 发送 CLOSE 0 后立即断线清理内存
    async fn kick(&self) -> Result<(), Box<dyn Error>> {
        self.send_close(0).await?;
        self.disconnect_now();
        Ok(())
    }

    /// 立即断线,清理内存
    pub fn disconnect_now(&self) {
        // 先关闭OPEN 0 标志位
        self.is_open_zero.store(false,Ordering::Release);

        if let Some(kcp_peer) = self.kcp_peer.upgrade() {
            //TODO 管它有没有 每个服务器都调用下 DropClientPeer 让服务器的 DropClientPeer 自己检查


            kcp_peer.disconnect();
            info!("peer:{} disconnect Cleanup", self.session_id);
        }
    }

    /// 发送数据
    pub async fn send(&self, session_id: u32, data: &[u8]) -> Result<(), Box<dyn Error>> {
        if let Some(kcp_peer) = self.kcp_peer.upgrade() {
            let mut writer = XBWrite::new();
            writer.put_u32_le(0);
            writer.put_u32_le(session_id);
            writer.write(data);
            writer.set_position(0);
            writer.put_u32_le(writer.len() as u32 - 4);
            kcp_peer.send(&writer).await?;
        }
        Ok(())
    }

    /// 发送OPEN
    pub async fn send_open(&self,service_id:u32)->  Result<(), Box<dyn Error>>{
        if let Some(kcp_peer) = self.kcp_peer.upgrade() {
            let mut writer = XBWrite::new();
            writer.put_u32_le(0);
            writer.put_u32_le(0xFFFFFFFF);
            writer.write_string_bit7_len("open");
            writer.bit7_write_u32(service_id);
            writer.set_position(0);
            writer.put_u32_le(writer.len()  as u32 - 4);
            kcp_peer.send(&writer).await?;
        }
        Ok(())
    }

    /// 发送CLOSE 0
    pub async fn send_close(&self, service_id: u32) -> Result<(), Box<dyn Error>> {
        if let Some(kcp_peer) = self.kcp_peer.upgrade() {
            let mut writer = XBWrite::new();
            writer.put_u32_le(0);
            writer.put_u32_le(0xFFFFFFFF);
            writer.write_string_bit7_len("close");
            writer.bit7_write_u32(service_id);
            writer.set_position(0);
            writer.put_u32_le(writer.len()  as u32 - 4);
            kcp_peer.send(&writer).await?;
        }
        Ok(())
    }

}

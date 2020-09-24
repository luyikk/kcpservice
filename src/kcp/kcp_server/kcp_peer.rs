use async_mutex::Mutex;
use crate::udp::TokenStore;
use super::super::kcp_module::{Kcp, KcpResult};
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::net::SocketAddr;
use log::*;
use std::cell::RefCell;


/// KCP LOCK
/// 将KCP 对象操作完全上锁,以保证内存安全 通知简化调用
pub struct KcpLock(Arc<Mutex<Kcp>>);
unsafe impl Send for KcpLock{}
unsafe impl Sync for KcpLock{}

impl KcpLock{
    #[inline]
    pub async fn peeksize(&self)-> KcpResult<usize>{
        self.0.lock_arc().await.peeksize()
    }

    #[inline]
    pub async fn check(&self,current:u32)->u32{
        self.0.lock_arc().await.check(current)
    }

    #[inline]
    pub async fn input(&self, buf: &[u8]) -> KcpResult<usize>{
        self.0.lock_arc().await.input(buf)
    }

    #[inline]
    pub async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize>{
        self.0.lock_arc().await.recv(buf)
    }

    #[inline]
    pub async fn send(&self, buf: &[u8]) -> KcpResult<usize>{
        self.0.lock_arc().await.send(buf)
    }

    #[inline]
    pub async fn update(&self, current: u32) ->  KcpResult<u32>{
        let mut p= self.0.lock_arc().await;
        p.update(current)?;
        Ok(p.check(current))
    }

    #[inline]
    pub async fn flush(&self) -> KcpResult<()>{
        self.0.lock_arc().await.flush()
    }

    #[inline]
    pub async fn flush_async(&self)->  KcpResult<()>{
        self.0.lock_arc().await.flush_async()
    }
}


impl Kcp{
    #[inline]
    pub fn get_lock(self)->KcpLock{
        let recv = Arc::new(Mutex::new(self));
        KcpLock(recv)
    }
}



/// KCP Peer
/// UDP的包进入 KCP PEER 经过KCP 处理后 输出
/// 输入的包 进入KCP PEER处理,然后 输出到UDP PEER SEND TO
/// 同时还需要一个UPDATE 线程 去10MS 一次的运行KCP UPDATE
/// token 用于扩赞逻辑上下文
pub struct KcpPeer<T: Send> {
    pub kcp:KcpLock,
    pub conv: u32,
    pub addr: SocketAddr,
    pub token: RefCell<TokenStore<T>>,
    pub last_rev_time: AtomicI64,
    pub next_update_time:AtomicU32,
    pub disconnect_event: RefCell<Option<Box<dyn FnOnce(u32)>>>
}

impl<T:Send> Drop for KcpPeer<T>{
    fn drop(&mut self) {
        info!("kcp_peer:{} is Drop",self.conv);
    }
}

unsafe impl<T: Send> Send for KcpPeer<T>{}
unsafe impl<T: Send> Sync for KcpPeer<T>{}

/// 简化KCP PEER 函数
impl <T:Send> KcpPeer<T>{

    pub fn disconnect(&self) {
        let call_value = self.disconnect_event.borrow_mut().take();
        if let Some(call) = call_value {
            call(self.conv);
        }
    }

    #[inline]
    pub async fn peeksize(&self)-> KcpResult<usize>{
        self.kcp.peeksize().await
    }

    #[inline]
    pub async fn input(&self, buf: &[u8]) -> KcpResult<usize>{
        self.next_update_time.store(0,Ordering::Release);
        self.kcp.input(buf).await
    }

    #[inline]
    pub async fn recv(&self, buf: &mut [u8]) -> KcpResult<usize>{
        self.kcp.recv(buf).await
    }

    #[inline]
    pub async fn send(&self, buf: &[u8]) -> KcpResult<usize>{
        self.next_update_time.store(0,Ordering::Release);
        self.kcp.send(buf).await
    }

    #[inline]
    pub async fn update(&self, current: u32) ->  KcpResult<()>{
        let next= self.kcp.update(current).await?;
        Ok(self.next_update_time.store(next+current,Ordering::Release))
    }

    #[inline]
    pub async fn flush(&self) -> KcpResult<()>{
        self.kcp.flush().await
    }

    #[inline]
    pub async fn flush_async(&self)->  KcpResult<()>{
        self.kcp.flush_async().await
    }

}
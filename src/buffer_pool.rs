use log::*;
use bytes::{Buf, BufMut};
use std::error::Error;
use tokio::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 数据包缓冲区
/// ```rust
///    let mut bp=buffer_pool::BuffPool::new(10);
///     bp.write(&[4,0,0,0,1,2,3,4]);
///     if let Some(p)= bp.read(){
///         println!("{:?}",p);
///     }
///
///     bp.write(&[4,0,0,0,4,3,2,1]);
///     if let Some(p)= bp.read(){
///         println!("{:?}",p);
///     }
///
///     bp.write(&[8,0,0,0,4,3,2,1,0,1,2,3]);
///     if let Some(p)= bp.read(){
///         println!("{:?}",p);
///     }
///
///     bp.write(&[10,0,0,0,4,3,2,1,0,1,2,3]);
///     if let Some(p)= bp.read(){
///         println!("{:?}",p);
///     }
///     else{
///         bp.advance();
///         println!("{:?}",bp);
///     }
///
///     bp.write(&[1,1]);
///     if let Some(p)= bp.read(){
///         println!("{:?}",p);
///     }
/// ```
#[derive(Debug)]
pub struct  BuffPool{
    data: RwLock<Vec<u8>>,
    offset:AtomicUsize,
    max_buff_size:usize
}

unsafe  impl  Send for BuffPool{}
unsafe  impl  Sync for BuffPool{}

impl BuffPool{

    pub fn new(capacity:usize)->BuffPool{
        let rw_lock= RwLock::new(Vec::with_capacity(capacity));
        BuffPool {
            data: rw_lock,
            offset: AtomicUsize::new(0),
            max_buff_size: capacity
        }
    }
    /// 写入 需要写锁
    pub async fn write(&self,data:&[u8]){
        let mut writer=self.data.write().await;
        writer.put_slice(data);
    }

    /// 读取 需要读锁
    pub async fn read(&self)->Result<Option<Vec<u8>>,&'static str>{
        let reader=self.data.read().await;
        let offset=self.offset.load(Ordering::Acquire);
        if offset+4>reader.len(){
            return  Ok(None)
        }
        let mut current=&reader[offset..];
        let len=current.get_u32_le() as usize;
        if len>self.max_buff_size{
            return Err("buff len too long")
        }
        if len>current.len(){
            return Ok(None)
        }
        let data= current[..len].to_vec();
        self.offset.store(offset+len+4,Ordering::Release);
        Ok(Some(data))
    }

    /// 挪数据 从屁股到头
    pub async fn advance(&self){
        let offset=self.offset.load(Ordering::Acquire);
        if offset==0{
            return;
        }
        let mut write=self.data.write().await;
        let len=write.len();
        unsafe {
            write.copy_within(offset..,0);
            write.set_len(len-offset);
            self.offset.store(offset,Ordering::Release);
        }
    }

    pub async fn reset(&self){
        let mut write=self.data.write().await;
        unsafe {
            write.set_len(0);
            self.offset.store(0,Ordering::Release);
        }
    }
}

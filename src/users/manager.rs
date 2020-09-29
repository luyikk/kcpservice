use super::client::ClientPeer;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use ClientHandleCmd::*;
use log::*;

pub enum ClientHandleCmd {
    CreatePeer(Arc<ClientPeer>),
    RemovePeer(u32),
    OpenPeer(u32,u32),
    ClosePeer(u32,u32),
    KickPeer(u32,u32,i32)
}

impl Debug for ClientHandleCmd {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CreatePeer(_) => f.debug_struct("CreatePeer").finish(),
            RemovePeer(_) => f.debug_struct("RemovePeer").finish(),
            OpenPeer(_,_)=> f.debug_struct("OpenPeer").finish(),
            ClosePeer(_,_)=>f.debug_struct("ClosePeer").finish(),
            KickPeer(_,_,_)=>f.debug_struct("KickPeer").finish(),
        }
    }
}

pub type ClientHandleError = Result<(), SendError<ClientHandleCmd>>;

#[derive(Clone)]
pub struct ClientHandle {
    tx: UnboundedSender<ClientHandleCmd>,
}

impl ClientHandle {
    pub fn new(tx: UnboundedSender<ClientHandleCmd>) -> ClientHandle {
        ClientHandle { tx }
    }

    ///创建客户端
    pub fn create_peer(&mut self, peer: Arc<ClientPeer>) -> ClientHandleError {
        self.tx.send(CreatePeer(peer))
    }

    ///删除PEER
    pub fn remove_peer(&mut self, conv: u32) -> ClientHandleError {
        self.tx.send(RemovePeer(conv))
    }

    ///服务器成功OPEN后的通知
    pub fn open_service(&mut self,service_id:u32,session_id:u32)->ClientHandleError{
        self.tx.send(OpenPeer(service_id,session_id))
    }
    /// CLOSE PEER
    pub fn close_peer(&mut self,service_id:u32,session_id:u32)->ClientHandleError{
        self.tx.send(ClosePeer(service_id,session_id))
    }
    /// 强制T
    pub fn kick_peer(&mut self,service_id:u32,session_id:u32,delay_ms:i32)->ClientHandleError{
        self.tx.send(KickPeer(service_id,session_id,delay_ms))
    }
}

///玩家peer管理服务
pub struct UserClientManager {
    users: RefCell<HashMap<u32, Arc<ClientPeer>>>,
    handle: ClientHandle,
}

unsafe impl Send for UserClientManager {}
unsafe impl Sync for UserClientManager {}

impl UserClientManager {
    /// 创建客户端管理器
    pub fn new() -> Arc<UserClientManager> {
        let (tx, rx) = unbounded_channel();
        let res = Arc::new(UserClientManager {
            users: RefCell::new(HashMap::new()),
            handle: ClientHandle::new(tx),
        });
        Self::recv(res.clone(), rx);
        res
    }

    ///获取PEER 只提供内部使用
    fn get_peer(&self, conv: &u32) -> Option<Arc<ClientPeer>> {
        self.users.borrow().get(conv).map(|x| x.clone())
    }

    /// 获取客户端管理器的操作句柄
    pub fn get_handle(&self) -> ClientHandle {
        self.handle.clone()
    }

    ///CSP 读取
    fn recv(manager: Arc<UserClientManager>, mut rx: UnboundedReceiver<ClientHandleCmd>) {
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match msg {
                    //收到创建客户端PEER 命令
                    CreatePeer(peer) => {
                        manager.users.borrow_mut().insert(peer.session_id, peer);
                    },
                    //删除peer
                    RemovePeer(conv) => {
                        manager.users.borrow_mut().remove(&conv);
                    },
                    //OPEN客户端
                    OpenPeer(service_id,session_id)=>{
                       if let Some(peer)=  manager.users.borrow().get(&session_id){
                            peer.open_service(service_id);
                       }
                    },
                    //完成此PEER
                    ClosePeer(service_id,session_id)=>{
                        if let Some(peer)=  manager.users.borrow().get(&session_id){
                            peer.close_service(service_id);
                        }
                    },
                    KickPeer(service_id,session_id,deley_ms)=>{
                        if let Some(peer)=  manager.get_peer(&session_id){
                            info!("service:{} kick peer:{}",service_id,session_id);
                            if let Err(err) = peer.kick_wait_ms(deley_ms).await {
                                error!("service:{} kick peer:{} is error:{:?}",service_id,session_id,err)
                            }
                        }
                    }
                }
            }
        });
    }
}

use super::client::ClientPeer;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use ClientHandleCmd::*;

pub enum ClientHandleCmd {
    CreatePeer(Arc<ClientPeer>),
    RemovePeer(u32),
}

impl Debug for ClientHandleCmd {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CreatePeer(_) => f.debug_struct("CreatePeer").finish(),
            RemovePeer(_) => f.debug_struct("RemovePeer").finish(),
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

    pub fn remove_peer(&mut self, conv: u32) -> ClientHandleError {
        self.tx.send(RemovePeer(conv))
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
                    }
                }
            }
        });
    }

}

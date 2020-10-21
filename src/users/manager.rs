use super::super::services::ServiceHandler;
use super::client::ClientPeer;
use log::*;
use std::cell::RefCell;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use xbinary::XBRead;
use ClientHandleCmd::*;
use ahash::AHashMap;

pub enum ClientHandleCmd {
    CreatePeer(Arc<ClientPeer>),
    RemovePeer(u32),
    OpenPeer(u32, u32),
    ClosePeer(u32, u32),
    KickPeer(u32, u32, i32),
    SendBuffer(u32, u32, XBRead),
    CloseAllPlayer(u32,Vec<u32>),
}

impl Debug for ClientHandleCmd {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CreatePeer(peer) => f
                .debug_struct("CreatePeer")
                .field("session_id", &peer.session_id)
                .finish(),
            RemovePeer(session_id) => f
                .debug_struct("RemovePeer")
                .field("session_id", session_id)
                .finish(),
            OpenPeer(service_id, session_id) => f
                .debug_struct("OpenPeer")
                .field("service_id", service_id)
                .field("session_id", session_id)
                .finish(),
            ClosePeer(service_id, session_id) => f
                .debug_struct("ClosePeer")
                .field("service_id", service_id)
                .field("session_id", session_id)
                .finish(),
            KickPeer(service_id, session_id, delay_ms) => f
                .debug_struct("KickPeer")
                .field("service_id", service_id)
                .field("session_id", session_id)
                .field("delay_ms", delay_ms)
                .finish(),
            SendBuffer(service_id, session_id, buff) => f
                .debug_struct("ClientSendBuffer")
                .field("service_id", service_id)
                .field("session_id", session_id)
                .field("buff", &buff.to_vec())
                .finish(),
            CloseAllPlayer(server_id,users) =>f
                .debug_struct("DropAllPlayer")
                .field("server_id",server_id)
                .field("users",users)
                .finish()
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

    /// 于服务器断开连接时通知客户端CLOSE
    pub fn close_all_user(&mut self,service_id:u32,users:Vec<u32>) -> ClientHandleError{
        self.tx.send(CloseAllPlayer(service_id,users))
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
    pub fn open_service(&mut self, service_id: u32, session_id: u32) -> ClientHandleError {
        self.tx.send(OpenPeer(service_id, session_id))
    }
    /// CLOSE PEER
    pub fn close_peer(&mut self, service_id: u32, session_id: u32) -> ClientHandleError {
        self.tx.send(ClosePeer(service_id, session_id))
    }
    /// 强制T
    pub fn kick_peer(
        &mut self,
        service_id: u32,
        session_id: u32,
        delay_ms: i32,
    ) -> ClientHandleError {
        self.tx.send(KickPeer(service_id, session_id, delay_ms))
    }
    /// 发送数据包
    pub fn send_buffer(
        &mut self,
        service_id: u32,
        session_id: u32,
        buff: XBRead,
    ) -> ClientHandleError {
        self.tx.send(SendBuffer(service_id, session_id, buff))
    }
}

///玩家peer管理服务
pub struct UserClientManager {
    users: RefCell<AHashMap<u32, Arc<ClientPeer>>>,
    handle: ClientHandle,
    service_handle: RefCell<Option<ServiceHandler>>,
}

unsafe impl Send for UserClientManager {}
unsafe impl Sync for UserClientManager {}

impl UserClientManager {
    /// 创建客户端管理器
    pub fn new() -> Arc<UserClientManager> {
        let (tx, rx) = unbounded_channel();
        let res = Arc::new(UserClientManager {
            users: RefCell::new(AHashMap::new()),
            handle: ClientHandle::new(tx),
            service_handle: RefCell::new(None),
        });
        Self::recv(res.clone(), rx);
        res
    }

    ///获取PEER 只提供内部使用
    fn get_peer(&self, conv: &u32) -> Option<Arc<ClientPeer>> {
        self.users.borrow().get(conv).cloned()
    }

    /// 获取客户端管理器的操作句柄
    pub fn get_handle(&self) -> ClientHandle {
        self.handle.clone()
    }

    /// 设置服务器句柄
    pub fn set_service_handler(&self, handler: ServiceHandler) {
        self.service_handle.borrow_mut().replace(handler);
    }

    /// 获取服务器句柄,如果没有直接 panic
    pub fn get_service_handler(&self) -> ServiceHandler {
        if let Some(ref handler) = *self.service_handle.borrow() {
            return handler.clone();
        }

        panic!("service handle is null");
    }

    ///CSP 读取
    fn recv(manager: Arc<UserClientManager>, mut rx: UnboundedReceiver<ClientHandleCmd>) {
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                match msg {
                    //收到创建客户端PEER 命令
                    CreatePeer(peer) => {
                        manager.users.borrow_mut().insert(peer.session_id, peer);
                    }
                    //删除peer
                    RemovePeer(conv) => {
                        if let Some(peer) = manager.users.borrow_mut().remove(&conv) {
                            if let Err(err) = peer.disconnect_now() {
                                error!("RemovePeer:{} is error:{}->{:?}", conv, err, err)
                            }
                        }
                    }
                    //OPEN客户端
                    OpenPeer(service_id, session_id) => {
                        if let Some(peer) = manager.get_peer(&session_id) {
                            if let Err(err) = peer.open_service(service_id).await {
                                error!(
                                    "service:{} open peer:{} is error:{}->{:?}",
                                    service_id, session_id, err, err
                                )
                            }
                        }
                    }
                    //完成此PEER
                    ClosePeer(service_id, session_id) => {
                        if let Some(peer) = manager.get_peer(&session_id) {
                            if let Err(err) = peer.close_service(service_id).await {
                                error!(
                                    "service:{} close peer:{} is error:{}->{:?}",
                                    service_id, session_id, err, err
                                )
                            }
                        }
                    }
                    //强制T此玩家
                    KickPeer(service_id, session_id, delay_ms) => {
                        if let Some(peer) = manager.get_peer(&session_id) {
                            info!("service:{} kick peer:{}", service_id, session_id);
                            if let Err(err) = peer.kick_wait_ms(delay_ms).await {
                                error!(
                                    "service:{} kick peer:{} is error:{}->{:?}",
                                    service_id, session_id, err, err
                                )
                            }
                        }
                    }
                    //转发给客户端数据
                    SendBuffer(service_id, session_id, buffer) => {
                        if let Some(peer) = manager.get_peer(&session_id) {
                            if let Err(err) = peer.send(service_id, &buffer).await {
                                error!(
                                    "service:{}  peer:{} send buffer error:{}->{:?}",
                                    service_id, session_id, err, err
                                )
                            }
                        }
                    },
                    CloseAllPlayer(service_id,users) =>{
                        for session_id in users {
                            if let Some(peer) = manager.get_peer(&session_id) {
                                if let Err(er) = peer.close_service(service_id).await {
                                    warn!("CloseAllPlayer service:{} peer:{} err:{}", service_id, session_id, er);
                                }
                            }
                        }
                    }
                }
            }

            error!("Client Manager is Drop!!");
        });
    }
}

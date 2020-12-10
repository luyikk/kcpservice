use super::super::users::ClientHandle;
use super::Service;
use ahash::AHashMap;
use bytes::Buf;
use json::JsonValue;
use log::*;
use std::cell::RefCell;
use std::error::Error;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::time::{delay_for, Duration};
use xbinary::XBRead;
use ServicesCmd::*;

/// 服务器操作命令
pub enum ServicesCmd {
    Disconnect(u32),
    OpenService(u32, u32, String),
    SendBuff(u32, u32, XBRead),
    DropClientPeer(u32),
    CheckPing,
}

impl Debug for ServicesCmd {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Disconnect(service_id) => f
                .debug_struct("Disconnect")
                .field("service_id", service_id)
                .finish(),
            OpenService(session_id, service_id, ipaddress) => f
                .debug_struct("OpenService")
                .field("session_id", session_id)
                .field("service_id", service_id)
                .field("ipaddress", ipaddress)
                .finish(),
            SendBuff(session_id, service_id, buffer) => f
                .debug_struct("SendBuff")
                .field("session_id", session_id)
                .field("service_id", service_id)
                .field("buff", &buffer.to_vec())
                .finish(),
            DropClientPeer(session_id) => f
                .debug_struct("DropClientPeer")
                .field("session_id", session_id)
                .finish(),
            CheckPing => f.debug_struct("CheckPing").finish(),
        }
    }
}

/// 服务器操作句柄
#[derive(Clone)]
pub struct ServiceHandler {
    tx: UnboundedSender<ServicesCmd>,
}

pub type ServiceHandlerError = Result<(), SendError<ServicesCmd>>;

impl ServiceHandler {
    /// 像服务器发起OPEN
    pub fn open(
        &mut self,
        session_id: u32,
        service_id: u32,
        ipaddress: String,
    ) -> ServiceHandlerError {
        self.tx.send(OpenService(session_id, service_id, ipaddress))
    }

    /// 发送数据包给服务器
    pub fn send_buffer(
        &mut self,
        session_id: u32,
        service_id: u32,
        buff: XBRead,
    ) -> ServiceHandlerError {
        self.tx.send(SendBuff(session_id, service_id, buff))
    }
    /// 断线通知
    pub fn disconnect_events(&mut self, session_id: u32) -> ServiceHandlerError {
        self.tx.send(DropClientPeer(session_id))
    }
}

/// 游戏服务器管理
pub struct ServicesManager {
    pub gateway_id: u32,
    pub service_cfg: JsonValue,
    services: RefCell<AHashMap<u32, Arc<Service>>>,
    handler: UnboundedSender<ServicesCmd>,
    client_handler: ClientHandle,
}

#[derive(Clone)]
pub struct ServiceManagerHandler(pub UnboundedSender<ServicesCmd>);

impl ServiceManagerHandler {
    pub fn disconnect(&self, service_id: u32) -> Result<(), SendError<ServicesCmd>> {
        self.0.send(Disconnect(service_id))
    }
}

unsafe impl Sync for ServicesManager {}
unsafe impl Send for ServicesManager {}

impl ServicesManager {
    pub fn new(
        config: &JsonValue,
        client_handler: ClientHandle,
    ) -> Result<Arc<ServicesManager>, Box<dyn Error>> {
        let (tx, mut rx) = unbounded_channel();

        let servers = ServicesManager {
            gateway_id: config["gatewayId"].as_u32().unwrap(),
            service_cfg: config["services"].clone(),
            services: RefCell::new(AHashMap::new()),
            handler: tx.clone(),
            client_handler,
        };

        for cfg in servers.service_cfg.members() {
            let service = Service::new(
                ServiceManagerHandler(tx.clone()),
                servers.client_handler.clone(),
                servers.gateway_id,
                cfg["serviceId"].as_u32().unwrap(),
                cfg["ip"].to_string(),
                cfg["port"].as_i32().unwrap(),
            );

            if servers
                .services
                .borrow_mut()
                .insert(service.service_id, Arc::new(service))
                .is_some()
            {
                return Err(format!(
                    "service_id: {} is have,check service_cfg.json",
                    cfg["serviceId"].as_u32().unwrap()
                )
                .into());
            }
        }

        let service_manager = Arc::new(servers);
        let inner_service_manager = service_manager.clone();
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Disconnect(service_id) => {
                        if let Some(service) = inner_service_manager.get_service(&service_id) {
                            if let Err(er) = service.disconnect().await {
                                error! {"disconnect service {} error:{}->{:?}",service_id,er,er}
                            }
                        } else {
                            error! {"disconnect not found service {} ",service_id}
                        }
                    }
                    OpenService(session_id, service_id, ipaddress) => {
                        if let Some(service) = inner_service_manager.get_service(&service_id) {
                            if let Err(er) = service.open(session_id, ipaddress).await {
                                error! {"open service {} session_id:{} error:{}->{:?}",service_id,session_id,er,er}
                            }
                        } else {
                            error! {"open service session_id:{} not found service {}",session_id,service_id}
                        }
                    }
                    //转发数据
                    SendBuff(session_id, service_id, mut buffer) => {
                        match service_id {
                            0xEEEEEEEE => {
                                let (size, serial) = buffer.read_bit7_i32();
                                if size > 0 {
                                    buffer.advance(size);
                                    let (size, typeid) = buffer.read_bit7_i32();
                                    if size > 0 {
                                        buffer.advance(size);
                                        if let Some(service) = inner_service_manager
                                            .get_service_by_typeid(session_id, typeid)
                                            .await
                                        {
                                            if let Err(er) = service.send_buffer_by_typeid(
                                                session_id, serial, typeid, &buffer,
                                            ) {
                                                error! {"sendbuff 0xEEEEEEEE error service {} session_id:{} typeid:{} error:{}->{:?}",service_id,session_id,typeid,er,er}
                                            }
                                        } else {
                                            error! {"sendbuff 0xEEEEEEEE not found service session_id:{} typeid:{}",session_id,typeid}
                                        }
                                        continue;
                                    }
                                }

                                error! {"sendbuff 0xEEEEEEEE error buffer session_id:{}",session_id}
                            }
                            _ => {
                                //需要指定转发的包
                                if let Some(service) =
                                    inner_service_manager.get_service(&service_id)
                                {
                                    if let Err(er) = service.send_buffer(session_id, &buffer) {
                                        error! {"sendbuff error service {} session_id:{} error:{}",service_id,session_id,er}
                                    }
                                }
                            }
                        }
                    }
                    //客户端断线
                    DropClientPeer(session_id) => {
                        let send_drop_services:Vec<Arc<Service>> = inner_service_manager.services.borrow().values()
                            .filter(|service|{
                                service.have_session_id(session_id)
                            })
                            .cloned()
                            .collect();

                        if send_drop_services.len() >0 {
                            for service in send_drop_services {
                                if let Err(er) = service.client_drop(session_id).await {
                                    error! {"DropClientPeer error service {} session_id:{} error:{}->{:?}", service.service_id, session_id, er, er}
                                }
                            }
                        }
                        else{
                            if let Some(service) = inner_service_manager.get_service(&0) {
                                if let Err(er) = service.client_drop(session_id).await {
                                    error! {"DropClientPeer error main service 0 session_id:{} error:{}->{:?}", session_id, er, er}
                                }
                            }
                        }
                    }
                    CheckPing => {
                        for service in inner_service_manager.services.borrow().values() {
                            service.check_ping();
                        }
                    }
                }
            }
        });

        tokio::spawn(async move {
            loop {
                if tx.send(CheckPing).is_err() {
                    break;
                }
                //每隔5秒发一次PING
                delay_for(Duration::from_secs(5)).await;
            }
        });

        Ok(service_manager)
    }

    /// 启动服务
    pub async fn start(&self) -> Result<(), Box<dyn Error>> {
        for service in self.services.borrow().values() {
            service.start().await;
        }
        Ok(())
    }

    /// 获取服务器管理句柄
    pub fn get_manager_handle(&self) -> ServiceManagerHandler {
        ServiceManagerHandler(self.handler.clone())
    }

    /// 获取服务器句柄
    pub fn get_handler(&self) -> ServiceHandler {
        ServiceHandler {
            tx: self.handler.clone(),
        }
    }

    /// 获取客户端句柄
    pub fn get_client_handle(&self) -> ClientHandle {
        self.client_handler.clone()
    }

    /// 获取服务器
    fn get_service(&self, service_id: &u32) -> Option<Arc<Service>> {
        self.services.borrow().get(&service_id).cloned()
    }

    /// 根据TYPEId 获取服务器
    async fn get_service_by_typeid(&self, session_id: u32, typeid: i32) -> Option<Arc<Service>> {
        for service in self.services.borrow().values() {
            if service.check_typeid(session_id, typeid) {
                return Some(service.clone());
            }
        }

        None
    }
}

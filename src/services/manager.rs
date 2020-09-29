use super::Service;
use ahash::AHashMap;
use json::JsonValue;
use std::cell::RefCell;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel,UnboundedSender};
use ServicesCmd::*;
use tokio::sync::mpsc::error::SendError;
use log::*;
use super::super::users::ClientHandle;

pub enum ServicesCmd{
    Disconnect(u32)
}

/// 游戏服务器管理
pub struct ServicesManager {
    pub gateway_id: u32,
    pub service_cfg: JsonValue,
    services: RefCell<AHashMap<u32, Arc<Service>>>,
    handler:UnboundedSender<ServicesCmd>,
    client_handler:ClientHandle
}

#[derive(Clone)]
pub struct ServiceManagerHandler(pub UnboundedSender<ServicesCmd>);

impl ServiceManagerHandler{
    pub fn disconnect(&self, service_id:u32) -> Result<(), SendError<ServicesCmd>> {
        self.0.send(Disconnect(service_id))
    }
}


unsafe impl Sync for ServicesManager {}
unsafe impl Send for ServicesManager {}

impl ServicesManager {
    pub fn new(config: &JsonValue,client_handler:ClientHandle) -> Result<Arc<ServicesManager>, Box<dyn Error>> {

        let (tx, mut rx)=unbounded_channel();

        let servers = ServicesManager {
            gateway_id: config["gatewayId"].as_u32().unwrap(),
            service_cfg: config["services"].clone(),
            services: RefCell::new(AHashMap::new()),
            handler:tx.clone(),
            client_handler
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


        let service_manager=Arc::new(servers);
        let inner_service_manager =service_manager.clone();
        tokio::spawn(async move{
            while let Some(cmd)=rx.recv().await {
                match cmd {
                    Disconnect(service_id)=>{
                       if let Some(service)= inner_service_manager.get_service(&service_id) {
                           if let Err(er)= service.disconnect().await{
                               error!{"disconnect service {} error:{}",service_id,er}
                           }
                       }
                       else {
                           error!{"disconnect not found service {} ",service_id}
                       }
                    }
                }

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

    /// 获取句柄
    pub fn get_handle(&self)->ServiceManagerHandler{
        ServiceManagerHandler(self.handler.clone())
    }

    /// 获取客户端句柄
    pub fn get_client_handle(&self)->ClientHandle{
        self.client_handler.clone()
    }

    fn get_service(&self, service_id:&u32) -> Option<Arc<Service>> {
        self.services.borrow().get(&service_id).map(|x|{x.clone()})
    }
}

use super::super::users::ClientHandle;
use super::connect::ConnectCmd;
use super::manager::ServiceManagerHandler;
use super::Connect;
use ahash::AHashSet;
use async_mutex::Mutex;
use bytes::{Buf, BufMut, Bytes};
use log::*;
use std::cell::{UnsafeCell};
use std::error::Error;
use std::io;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::{delay_for, Duration};
use xbinary::{XBRead, XBWrite};

///用于存放发送句柄
pub struct Sender(UnsafeCell<Option<UnboundedSender<XBWrite>>>);

unsafe impl Send for Sender {}
unsafe impl Sync for Sender {}

impl Sender {
    pub fn new() -> Sender {
        Sender(UnsafeCell::new(None))
    }

    pub fn get(&self) -> Option<UnboundedSender<XBWrite>> {
        unsafe {
            if let Some(ref p) = *self.0.get() {
                Some(p.clone())
            } else {
                None
            }
        }
    }

    pub fn set(&self, p: UnboundedSender<XBWrite>) {
        unsafe {
            *self.0.get() = Some(p);
        }
    }

    pub fn clean(&self) {
        unsafe {
            *self.0.get() = None;
        }
    }

    pub fn send(&self, data: XBWrite) -> Result<(), Box<dyn Error>> {
        if let Some(sender) = self.get() {
            sender.send(data)?;
            Ok(())
        } else {
            Err("not found sender tx, check connect".into())
        }
    }
}


pub struct ServiceInner {
    pub gateway_id: u32,
    pub manager_handle: ServiceManagerHandler,
    pub connect: Arc<Mutex<Option<Connect>>>,
    pub msg_ids: UnsafeCell<Vec<i32>>,
    pub sender: Arc<Sender>,
    pub last_ping_time: AtomicI64,
    pub ping_delay_tick: AtomicI64,
    pub client_handle: ClientHandle,
    pub wait_open_table: Mutex<AHashSet<u32>>,
    pub open_table: UnsafeCell<AHashSet<u32>>,
}

unsafe impl Send for ServiceInner {}
unsafe impl Sync for ServiceInner {}

/// 游戏服务器对象
pub struct Service {
    pub service_id: u32,
    pub ip: String,
    pub port: i32,
    inner: Arc<ServiceInner>,
}
unsafe impl Send for Service {}
unsafe impl Sync for Service {}

impl Service {
    pub fn new(
        handler: ServiceManagerHandler,
        client_handle: ClientHandle,
        gateway_id: u32,
        service_id: u32,
        ip: String,
        port: i32,
    ) -> Service {
        Service {
            service_id,
            ip,
            port,
            inner: Arc::new(ServiceInner {
                gateway_id,
                manager_handle: handler,
                connect: Arc::new(Mutex::new(None)),
                msg_ids: UnsafeCell::new(Vec::new()),
                sender: Arc::new(Sender::new()),
                last_ping_time: AtomicI64::new(0),
                ping_delay_tick: AtomicI64::new(0),
                wait_open_table: Mutex::new(AHashSet::new()),
                open_table: UnsafeCell::new(AHashSet::new()),
                client_handle,
            }),
        }
    }

    /// 启动
    pub async fn start(&self) {
        info!("service:{} is start", self.service_id);
        self.try_connect(false).await;
    }

    /// 尝试连接
    async fn try_connect(&self, need_wait: bool) {
        let service_id = self.service_id;
        let ip = self.ip.clone();
        let port = self.port;
        let inner = self.inner.clone();
        tokio::spawn(async move {
            if need_wait {
                delay_for(Duration::from_secs(5)).await;
            }

            loop {
                match Self::connect(inner.manager_handle.clone(), service_id, ip.clone(), port)
                    .await
                {
                    Ok((mut connect, sender)) => {
                        let mut reader = connect.read_rx.take().unwrap();
                        inner.connect.lock_arc().await.replace(connect);
                        inner.sender.set(sender);
                        info!("connect to {}-{}:{} OK", service_id, ip, port);

                        if let Err(er) = Self::send_register(inner.gateway_id, &inner.sender) {
                            error!("register {} gateway error:{:?}", service_id, er);
                            break;
                        }

                        // 开始读取数据
                        while let Some(data) = reader.recv().await {
                            match data {
                                ConnectCmd::Buff(data) => {
                                    if let Err(er) = Self::read_data(data, service_id, &inner).await
                                    {
                                        error!("read data error:{:?}", er);
                                        break;
                                    }
                                }
                                ConnectCmd::DropClient(session_id) => {
                                    inner.wait_open_table.lock().await.remove(&session_id);
                                    unsafe {
                                        (*inner.open_table.get()).remove(&session_id);
                                    }

                                    info!(
                                        "disconnect peer:{} to service:{}",
                                        session_id, service_id
                                    );
                                    if let Some(sender) = inner.sender.get() {
                                        if let Err(er) =
                                        Self::send_disconnect(session_id, sender)
                                        {
                                            error!(
                                                "send disconnect to service:{} error:{:?}",
                                                service_id, er
                                            );
                                        }
                                    }
                                },
                                ConnectCmd::Disconnect=>{
                                    unsafe {
                                        warn!("service:{} disconnect start close all users",service_id);

                                        let need_close_ids:Vec<u32>=(*inner.open_table.get()).iter().copied().collect();

                                        (*inner.open_table.get()).clear();

                                        if let Err(er)= inner.client_handle.clone().close_all_user(service_id,need_close_ids){
                                            error!("service:{} disconnect close all user err{}",service_id,er);
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                        break;
                    }
                    Err(er) => {
                        error!(
                            "connect to {}-{}:{} fail:{};restart in 5 seconds",
                            service_id, ip, port, er
                        );
                    }
                }

                delay_for(Duration::from_secs(5)).await;
            }
        });
    }

    /// 连接
    #[inline]
    async fn connect(
        handler: ServiceManagerHandler,
        service_id: u32,
        ip: String,
        port: i32,
    ) -> io::Result<(Connect, UnboundedSender<XBWrite>)> {
        Connect::new(
            format!("{}:{}", ip, port),
            Box::new(move || handler.disconnect(service_id)),
        )
        .await
    }

    /// 断线
    pub async fn disconnect(&self) -> Result<(), Box<dyn Error>> {
        info!(
            "service:{}-{}-{} disconnect start reconnect",
            self.service_id, self.ip, self.port
        );
        self.inner.last_ping_time.store(0, Ordering::Release);
        self.inner.sender.clean();
        self.inner.connect.lock_arc().await.take();
        self.try_connect(true).await;
        Ok(())
    }

    /// PING检测
    pub fn check_ping(&self) {
        if let Some(sender) = self.inner.sender.get() {
            let last_ping_time = self.inner.last_ping_time.load(Ordering::Acquire);
            let now = Self::timestamp();

            //30秒超时 单位tick 秒后 7个0
            if last_ping_time > 0 && now - last_ping_time > 30 * 1000 * 10000 {
                warn!("service:{} ping time out,shutdown it,now:{},last_ping_time:{},ping_delay_tick:{}",
                      self.service_id,
                      now,
                      last_ping_time,
                      self.inner.ping_delay_tick.load(Ordering::Acquire));

                if let Err(er) = sender.send(XBWrite::new()) {
                    error!(
                        "service{} ping time out,need disconnect error:{}->{:?}",
                        self.service_id, er, er
                    );
                }
            } else if let Err(er) = Self::send_ping(now, sender) {
                error!(
                    "service{} send ping  error:{}->{:?}",
                    self.service_id, er, er
                );
            }
        }
    }

    /// 发送数据
    pub fn send(&self, data: XBWrite) -> Result<(), Box<dyn Error>> {
        self.inner.sender.send(data)
    }

    /// 读取数据
    async fn read_data(
        data: Vec<u8>,
        service_id: u32,
        inner: &Arc<ServiceInner>,
    ) -> Result<(), Box<dyn Error>> {
        let mut reader = XBRead::new(Bytes::from(data));
        let session_id = reader.get_u32_le();
        if session_id == 0xFFFFFFFFu32 {
            //到网关的数据
            let cmd = reader.read_string_bit7_len();
            match cmd {
                None => return Err(format!("service:{} not read cmd", service_id).into()),
                Some(cmd) => {
                    match &cmd[..] {
                        "typeids" => {
                            let len = reader.get_u32_le();
                            unsafe {
                                for _ in 0..len {
                                    (*inner.msg_ids.get()).push(reader.get_i32_le());
                                }
                            }
                            info!("Service:{} push typeids count:{}", service_id, len);
                        }
                        "ping" => {
                            let tick = reader.read_bit7_i64();
                            let now = Self::timestamp();
                            if tick.0 > 0 {
                                reader.advance(tick.0);
                                inner.ping_delay_tick.store(now - tick.1, Ordering::Release);
                                inner.last_ping_time.store(now, Ordering::Release);
                            } else {
                                warn!("service:{} read ping tick fail", service_id);
                                inner.last_ping_time.store(now, Ordering::Release);
                            }
                        }
                        "open" => {
                            let session_id = reader.read_bit7_u32();
                            if session_id.0 > 0 {
                                reader.advance(session_id.0);
                                let session_id = session_id.1;
                                if session_id == 0 {
                                    //如果是0号服务器需要到表里面查询一番 查不到打警告返回
                                    if !inner.wait_open_table.lock().await.remove(&session_id) {
                                        warn!("service:{} not found SessionId:{} open is fail,Maybe the client is disconnected.", service_id, session_id);
                                        return Ok(());
                                    }
                                }

                                unsafe {
                                    if (*inner.open_table.get()).insert(session_id) {
                                        inner
                                            .client_handle
                                            .clone()
                                            .open_service(service_id, session_id)?;
                                    } else {
                                        warn!(
                                            "service: {} insert SessionId:{} open is fail",
                                            service_id, session_id
                                        );
                                    }
                                }
                            } else {
                                return Err(format!(
                                    "service:{} read open session_id fail",
                                    service_id
                                )
                                .into());
                            }
                        }
                        "close" => {
                            let session_id = reader.read_bit7_u32();
                            if session_id.0 > 0 {
                                reader.advance(session_id.0);
                                let session_id = session_id.1;
                                // 如果TRUE 说明还没OPEN 就被CLOSE了
                                if !inner.wait_open_table.lock().await.remove(&session_id) {
                                    unsafe {
                                        if !(*inner.open_table.get()).remove(&session_id) {
                                            //如果OPEN表里面找不到那么打警告返回
                                            warn!(
                                                "service:{} not found SessionId:{} close is fail",
                                                service_id, session_id
                                            );
                                            return Ok(());
                                        }
                                    }
                                }
                                inner
                                    .client_handle
                                    .clone()
                                    .close_peer(service_id, session_id)?;
                            } else {
                                return Err(
                                    format!("service:{} read close is fail", service_id).into()
                                );
                            }
                        }
                        "kick" => {
                            let session_id = reader.read_bit7_u32();
                            if session_id.0 > 0 {
                                reader.advance(session_id.0);
                                let session_id = session_id.1;
                                let delay_ms = reader.read_bit7_i32();
                                if delay_ms.0 > 0 {
                                    reader.advance(delay_ms.0);
                                    let delay_ms = delay_ms.1;
                                    inner
                                        .client_handle
                                        .clone()
                                        .kick_peer(service_id, session_id, delay_ms)?;
                                } else {
                                    return Err(format!(
                                        "service:{} read kick delay is fail",
                                        service_id
                                    )
                                    .into());
                                }
                            } else {
                                return Err(
                                    format!("service:{} read kick is fail", service_id).into()
                                );
                            }
                        }
                        _ => {
                            return Err(
                                format!("service:{} incompatible cmd:{}", service_id, cmd).into()
                            );
                        }
                    }
                }
            }
        } else {
            inner
                .client_handle
                .clone()
                .send_buffer(service_id, session_id, reader)?;
        }
        Ok(())
    }

    /// 发起OPEN请求
    pub async fn open(&self, session_id: u32, ipaddress: String) -> Result<(), Box<dyn Error>> {
        let mut wait_open_dict = self.inner.wait_open_table.lock().await;
        if wait_open_dict.insert(session_id) {
            if let Err(er) = Self::send_open(session_id, ipaddress, &self.inner.sender) {
                wait_open_dict.remove(&session_id);
                return Err(er);
            }
            return Ok(());
        }

        Err(format!("repeat open:{}", session_id).into())
    }

    /// 客户端断线调用
    pub async fn client_drop(&self, session_id: u32) -> Result<(), Box<dyn Error>> {
        let connect = self.inner.connect.lock_arc().await;
        if let Some(ref connect) = *connect {
            connect
                .read_tx
                .clone()
                .send(ConnectCmd::DropClient(session_id))
                .await?;
        }
        Ok(())
    }

    /// 此peer是否在此服务器OPEN
    pub fn have_session_id(&self, session_id: u32) -> bool {
        unsafe {
            if (*self.inner.open_table.get()).contains(&session_id) {
                return true;
            }
            false
        }
    }

    /// 检测此session_id 和 typeid 是否是此服务器
    pub fn check_typeid(&self, session_id: u32, typeid: i32) -> bool {
        unsafe {
            if (*self.inner.open_table.get()).contains(&session_id)
                && (*self.inner.msg_ids.get()).contains(&typeid)
            {
                return true;
            }
            false
        }
    }

    /// 发送BUFF 智能路由用
    #[inline]
    pub fn send_buffer_by_typeid(
        &self,
        session_id: u32,
        serial: i32,
        typeid: i32,
        buffer: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let mut writer = XBWrite::new();
        writer.put_u32_le(0);
        writer.put_u32_le(session_id);
        writer.bit7_write_i32(serial);
        writer.bit7_write_i32(typeid);
        writer.write(buffer);
        writer.set_position(0);
        writer.put_u32_le(writer.len() as u32 - 4);
        self.inner.sender.send(writer)?;
        Ok(())
    }

    /// 发送BUFF
    #[inline]
    pub fn send_buffer(&self, session_id: u32, buffer: &[u8]) -> Result<(), Box<dyn Error>> {
        let mut writer = XBWrite::new();
        writer.put_u32_le(0);
        writer.put_u32_le(session_id);
        writer.write(buffer);
        writer.set_position(0);
        writer.put_u32_le(writer.len() as u32 - 4);
        self.inner.sender.send(writer)?;
        Ok(())
    }

    /// 发送OPEN
    #[inline]
    fn send_open(
        session_id: u32,
        ipaddress: String,
        sender: &Arc<Sender>,
    ) -> Result<(), Box<dyn Error>> {
        let mut writer = XBWrite::new();
        writer.put_u32_le(0);
        writer.put_u32_le(0xFFFFFFFFu32);
        writer.write_string_bit7_len("accept");
        writer.bit7_write_u32(session_id);
        writer.write_string_bit7_len(&ipaddress);
        writer.set_position(0);
        writer.put_u32_le(writer.len() as u32 - 4);
        sender.send(writer)?;
        Ok(())
    }

    /// 发送注册网关
    #[inline]
    fn send_register(gateway_id: u32, sender: &Arc<Sender>) -> Result<(), Box<dyn Error>> {
        let mut writer = XBWrite::new();
        writer.put_u32_le(0);
        writer.put_u32_le(0xFFFFFFFFu32);
        writer.write_string_bit7_len("gatewayId");
        writer.bit7_write_u32(gateway_id);
        writer.put_i8(1);
        writer.set_position(0);
        writer.put_u32_le(writer.len() as u32 - 4);
        sender.send(writer)?;
        Ok(())
    }

    /// 发送断线
    #[inline]
    fn send_disconnect(
        session_id: u32,
        sender: UnboundedSender<XBWrite>,
    ) -> Result<(), Box<dyn Error>> {
        let mut writer = XBWrite::new();
        writer.put_u32_le(0);
        writer.put_u32_le(0xFFFFFFFFu32);
        writer.write_string_bit7_len("disconnect");
        writer.bit7_write_u32(session_id);
        writer.set_position(0);
        writer.put_u32_le(writer.len() as u32 - 4);
        sender.send(writer)?;
        Ok(())
    }

    /// 发送PING包
    #[inline]
    fn send_ping(time: i64, sender: UnboundedSender<XBWrite>) -> Result<(), Box<dyn Error>> {
        let mut writer = XBWrite::new();
        writer.put_u32_le(0);
        writer.put_u32_le(0xFFFFFFFFu32);
        writer.write_string_bit7_len("ping");
        writer.bit7_write_i64(time);
        writer.set_position(0);
        writer.put_u32_le(writer.len() as u32 - 4);
        sender.send(writer)?;
        Ok(())
    }

    /// 获取时间戳
    #[inline]
    fn timestamp() -> i64 {
        chrono::Local::now().timestamp_nanos() / 100
    }
}

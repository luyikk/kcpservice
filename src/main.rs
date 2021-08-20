#![feature(async_closure,cursor_remaining)]
#![allow(dead_code)]

mod buffer_pool;
mod kcp;
mod services;
mod stdout_log;
mod udp;
mod users;

use crate::kcp::{KcpConfig, KcpListener, KcpNoDelayConfig, KcpPeer};

use services::ServicesManager;
use stdout_log::StdErrLog;
use users::*;
use std::sync::Arc;
use bytes::Buf;
use flexi_logger::{Age, Cleanup, Criterion, LogTarget, Naming};
use json::JsonValue;
use lazy_static::lazy_static;
use mimalloc::MiMalloc;
use anyhow::*;
use structopt::*;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

lazy_static! {

    /// 配置文件
    pub static ref SERVICE_CFG:JsonValue={
        if let Ok(json)= std::fs::read_to_string("./service_cfg.json") {
            json::parse(&json).unwrap()
        }
        else{
            panic!("not found service_cfg.json");
        }
    };

     /// 用户管理
    pub static ref USER_PEER_MANAGER: Arc<UserClientManager> = UserClientManager::new();

     /// 服务管理
    pub static ref SERVICE_MANAGER:Arc<ServicesManager>=ServicesManager::new(&SERVICE_CFG,USER_PEER_MANAGER.get_handle()).unwrap();


}

#[tokio::main]
async fn main() -> Result<()> {
    init_log_system();
    SERVICE_MANAGER.start().await?;
    USER_PEER_MANAGER.set_service_handler(SERVICE_MANAGER.get_handler());

    let timeout_second = SERVICE_CFG["clientTimeoutSeconds"].as_i64().unwrap();
    let config=KcpConfig{
        nodelay: Some(KcpNoDelayConfig::fastest()),
        ..Default::default()
    };

    let kcp =
        KcpListener::<Arc<ClientPeer>, _>::new(format!("0.0.0.0:{}",SERVICE_CFG["listenPort"].as_i32().unwrap()), config, timeout_second).await?;

    kcp.set_kcpdrop_event_input(|conv| {
        let mut handle = USER_PEER_MANAGER.get_handle();
        handle.remove_peer(conv).unwrap();
    })
    .await;

    kcp.set_buff_input(async move |kcp_peer, mut data| {
        let peer = {
            let mut token = kcp_peer.token.borrow_mut();
            match token.get() {
                None => {
                    if data.len() >= 5 && data.get_u32_le() == 1 && data[0] == 0 {
                        data.advance(1);
                        let mut handle = USER_PEER_MANAGER.get_handle();
                        let service_handler = USER_PEER_MANAGER.get_service_handler();
                        let peer = Arc::new(ClientPeer::new(
                            kcp_peer.conv,
                            Arc::downgrade(&kcp_peer),
                            service_handler,
                        ));
                        handle.create_peer(peer.clone())?;
                        token.set(Some(peer.clone()));
                        peer.open(0)?;
                        peer
                    } else {
                        return Ok(());
                    }
                }
                Some(peer) => peer.clone(),
            }
        };

        peer.input_buff(&data).await?;
        Ok(())
    });

    kcp.start().await?;
    Ok(())
}

#[derive(StructOpt, Debug)]
#[structopt(name = "tcp gateway server")]
#[structopt(version=version())]
struct Opt{
    /// 是否显示 日志 到控制台
    #[structopt(short, long)]
    stdlog:bool,
    /// 是否打印崩溃堆栈
    #[structopt(short, long)]
    backtrace:bool
}



/// 安装日及系统
fn init_log_system() {
    let opt=Opt::from_args();
    if opt.backtrace{
        std::env::set_var("RUST_BACKTRACE","1");
    }

    let mut show_std =opt.stdlog;
    for (name, arg) in std::env::vars() {
        if name.trim() == "STDLOG" && arg.trim() == "1" {
            show_std = true;
            println!("open stderr log out");
        }
    }

    let mut log_set = LogTarget::File;
    if show_std {
        log_set = LogTarget::FileAndWriter(Box::new(StdErrLog::new()));
    }

    flexi_logger::Logger::with_str("debug")
        .log_target(log_set)
        .suffix("log")
        .directory("logs")
        .rotate(
            Criterion::AgeOrSize(Age::Day, 1024 * 1024 * 5),
            Naming::Numbers,
            Cleanup::KeepLogFiles(30),
        )
        .print_message()
        .format(flexi_logger::opt_format)
        .set_palette("196;190;6;7;8".into())
        .start()
        .unwrap();
}

#[inline(always)]
fn version()->&'static str{
    concat! {
    "\n",
    "==================================version info==================================",
    "\n",
    "Build Timestamp:", env!("VERGEN_BUILD_TIMESTAMP"), "\n",
    "GIT BRANCH:", env!("VERGEN_GIT_BRANCH"), "\n",
    "GIT COMMIT DATE:", env!("VERGEN_GIT_COMMIT_TIMESTAMP"), "\n",
    "GIT SHA:", env!("VERGEN_GIT_SHA"), "\n",
    "PROFILE:", env!("VERGEN_CARGO_PROFILE"), "\n",
    "==================================version end==================================",
    "\n",
    }
}
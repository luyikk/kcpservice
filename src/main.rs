#![feature(async_closure,cursor_remaining)]
#![allow(dead_code)]

mod buffer_pool;
mod kcp;
mod services;
mod stdout_log;
mod udp;
mod users;
mod config;

use crate::kcp::{KcpConfig, KcpListener, KcpNoDelayConfig, KcpPeer};

use services::ServicesManager;
use users::*;
use std::sync::Arc;
use bytes::Buf;
use lazy_static::lazy_static;
use mimalloc::MiMalloc;
use anyhow::Result;
use structopt::*;
use std::path::Path;
use std::env::current_dir;
use crate::config::Config;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

lazy_static! {
 /// 当前运行路径
    pub static ref CURRENT_EXE_PATH:String={
         match std::env::current_exe(){
            Ok(path)=>{
                if let Some(current_exe_path)= path.parent(){
                    return current_exe_path.to_string_lossy().to_string()
                }
                panic!("current_exe_path get error: is none");
            },
            Err(err)=> panic!("current_exe_path get error:{:?}",err)
        }
    };

    /// 加载网关配置
    pub static ref CONFIG:Config={
       let json_path= {
            let json_path=format!("{}/service_cfg.json", CURRENT_EXE_PATH.as_str());
            let path=Path::new(&json_path);
            if !path.exists(){
                let json_path=format!("{}/service_cfg.json", current_dir()
                    .expect("not found current dir")
                    .display());
                let path=Path::new(&json_path);
                if !path.exists(){
                     panic!("not found config file:{:?}",path);
                }else{
                    json_path
                }
            }
            else { json_path }
        };
        let path=Path::new(&json_path);
        serde_json::from_str::<Config>(&std::fs::read_to_string(path)
            .expect("not read service_cfg.json"))
            .expect("read service_cfg.json error")
    };
     /// 用户管理
    pub static ref USER_PEER_MANAGER: Arc<UserClientManager> = UserClientManager::new();

     /// 服务管理
    pub static ref SERVICE_MANAGER:Arc<ServicesManager>=ServicesManager::new(USER_PEER_MANAGER.get_handle()).unwrap();


}

#[tokio::main]
async fn main() -> Result<()> {
    install_log()?;
    SERVICE_MANAGER.start().await?;
    USER_PEER_MANAGER.set_service_handler(SERVICE_MANAGER.get_handler());

    let timeout_second = CONFIG.client_timeout_seconds as i64;
    let config=KcpConfig{
        nodelay: Some(KcpNoDelayConfig::fastest()),
        ..Default::default()
    };

    let kcp =
        KcpListener::<Arc<ClientPeer>, _>::new(format!("0.0.0.0:{}",CONFIG.listen_port), config, timeout_second).await?;

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
struct NavOpt{
    /// 是否显示 日志 到控制台
    #[structopt(short, long)]
    syslog:bool,
    /// 是否打印崩溃堆栈
    #[structopt(short, long)]
    backtrace:bool
}

#[inline(always)]
fn version() -> &'static str {
    concat! {
    "\n",
    "==================================version info=================================",
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


#[cfg(all(feature = "flexi_log", not(feature = "env_log")))]
static LOGGER_HANDLER: tokio::sync::OnceCell<flexi_logger::LoggerHandle> =
    tokio::sync::OnceCell::const_new();

fn install_log() -> Result<()> {
    let opt = NavOpt::from_args();
    if opt.backtrace {
        std::env::set_var("RUST_BACKTRACE", "1");
    }

    #[cfg(all(feature = "flexi_log", not(feature = "env_log")))]
    {
        use flexi_logger::{Age, Cleanup, Criterion, FileSpec, Logger, Naming, WriteMode};

        if opt.syslog {
            let logger = Logger::try_with_str("trace, sqlx = error,mio=error")?
                .log_to_file_and_writer(
                    FileSpec::default()
                        .directory("logs")
                        .suppress_timestamp()
                        .suffix("log"),
                    Box::new(stdout_log::StdErrLog),
                )
                .format(flexi_logger::opt_format)
                .rotate(
                    Criterion::AgeOrSize(Age::Day, 1024 * 1024 * 5),
                    Naming::Numbers,
                    Cleanup::KeepLogFiles(30),
                )
                .print_message()
                .set_palette("196;190;2;4;8".into())
                .write_mode(WriteMode::Async)
                .start()?;
            LOGGER_HANDLER
                .set(logger)
                .map_err(|_| anyhow::anyhow!("logger set error"))?;
        } else {
            let logger = Logger::try_with_str("trace, sqlx = error,mio = error")?
                .log_to_file(
                    FileSpec::default()
                        .directory("logs")
                        .suppress_timestamp()
                        .suffix("log"),
                )
                .format(flexi_logger::opt_format)
                .rotate(
                    Criterion::AgeOrSize(Age::Day, 1024 * 1024 * 5),
                    Naming::Numbers,
                    Cleanup::KeepLogFiles(30),
                )
                .print_message()
                .write_mode(WriteMode::Async)
                .start()?;
            LOGGER_HANDLER
                .set(logger)
                .map_err(|_| anyhow::anyhow!("logger set error"))?;
        }
    }
    #[cfg(all(feature = "flexi_log", feature = "env_log"))]
    {
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Trace)
            .filter_module("mio::poll", log::LevelFilter::Error)
            .init();
    }

    Ok(())
}

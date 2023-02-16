use serde::Deserialize;

/// 网关配置结构
#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(rename = "gatewayId")]
    pub gateway_id: u32,
    #[serde(rename = "listenPort")]
    pub listen_port: i32,
    #[serde(rename = "clientTimeoutSeconds")]
    pub client_timeout_seconds: i32,
    pub services: Vec<ServiceConfig>,
}

#[derive(Deserialize, Debug,Clone)]
pub struct ServiceConfig {
    #[serde(rename = "serviceId")]
    pub service_id: u32,
    pub ip: String,
    pub port: i32,
}

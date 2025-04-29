// 文件: src/config.rs
// ```rust
use serde::{Deserialize, Deserializer, Serialize}; // 修复：导入 serde
use std::sync::Arc;
use lazy_static::lazy_static;
use std::{env, fs::File, io::Read};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub bot: BotConfig,
    pub routing: RoutingConfig,
    pub rpc: RpcConfig,
    pub spam: Option<SpamConfig>,
    pub wallet: WalletConfig,
    pub kamino_flashloan: Option<KaminoFlashloanConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BotConfig {
    pub compute_unit_limit: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RoutingConfig {
    pub mint_config_list: Vec<MintConfig>,
}

#[derive(Debug, Serialize,Deserialize,Clone)]
pub struct MintConfig {
    pub mint: String,
    #[serde(default)] // 确保 null 或缺失时初始化为空数组
    pub raydium_pool_list: Vec<String>,
    #[serde(default)] // 确保 null 或缺失时初始化为空数组
    pub meteora_dlmm_pool_list: Vec<String>,
    #[serde(default)] // 确保 null 或缺失时初始化为空数组
    pub raydium_cp_pool_list: Vec<String>,
    #[serde(default)] // 确保 null 或缺失时初始化为空数组
    pub pump_pool_list: Vec<String>,
    #[serde(default)] // 确保 null 或缺失时初始化为空数组
    pub whirlpool_pool_list: Vec<String>,
    #[serde(default)] // 确保 null 或缺失时初始化为空数组
    pub raydium_clmm_pool_list: Vec<String>,
    #[serde(default="default_lookup_table")] // 确保 null 或缺失时初始化为空数组
    pub lookup_table_accounts: Vec<String>,
    #[serde(default="default_process_delay")] // 确保 null 或缺失时初始化为0
    pub process_delay: u64,
}
fn default_lookup_table() -> Vec<String> {
    vec!["4sKLJ1Qoudh8PJyqBeuKocYdsZvxTcRShUt9aKqwhgvC".to_string()]  // Default to an empty vector
}

fn default_process_delay() -> u64 {
    200
}

impl MintConfig {
    // 获取所有池地址
    pub  fn get_all_pool_addresses(&self) -> Vec<&String> {
        let mut addresses = vec![];
        addresses.extend(&self.meteora_dlmm_pool_list);
        addresses.extend(&self.pump_pool_list);
        addresses.extend(&self.raydium_cp_pool_list);
        addresses.extend(&self.raydium_pool_list);
        addresses.extend(&self.whirlpool_pool_list);
        addresses.extend(&self.raydium_clmm_pool_list);
        addresses
    }

    pub fn get_all_mint_addresses(&self) -> Vec<&String> {
        let mut addresses = vec![];
        addresses.push(&self.mint);
        addresses
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct RpcConfig {
    #[serde(deserialize_with = "serde_string_or_env")]
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SpamConfig {
    pub enabled: bool,
    pub sending_rpc_urls: Vec<String>,
    pub compute_unit_price: ComputeUnitPrice,
    pub max_retries: Option<u64>,
}
#[derive(Debug, Deserialize, Clone)]
pub struct ComputeUnitPrice {
    pub strategy: String,
    pub from: u64,
    pub to: u64,
    pub count: u64,
}
#[derive(Debug, Deserialize, Clone)]
pub struct WalletConfig {
    #[serde(default,deserialize_with = "serde_string_or_env")]
    pub private_key: String,
}
impl Default for WalletConfig {
    fn default() -> Self {
        WalletConfig {
            private_key: String::new(), // 默认值为空字符串
        }
    }
}
#[derive(Debug, Deserialize, Clone)]
pub struct KaminoFlashloanConfig {
    pub enabled: bool,
}

pub fn serde_string_or_env<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value_or_env = String::deserialize(deserializer)?;
    let value = match value_or_env.chars().next() {
        Some('$') => env::var(&value_or_env[1..])
            .unwrap_or_else(|_| panic!("reading `{}` from env", &value_or_env[1..])),
        _ => value_or_env,
    };
    Ok(value)
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn get_wali_config(&self) -> Arc<crate::wali_config::Config> {
        crate::wali_config::Config::get()
    }
}
impl Default for MintConfig {
    fn default() -> Self {
        MintConfig {
            mint: String::new(),
            raydium_pool_list: vec![],
            meteora_dlmm_pool_list: vec![],
            raydium_cp_pool_list: vec![],
            pump_pool_list: vec![],
            whirlpool_pool_list: vec![],
            raydium_clmm_pool_list: vec![],
            lookup_table_accounts: vec!["4sKLJ1Qoudh8PJyqBeuKocYdsZvxTcRShUt9aKqwhgvC".to_string()],
            process_delay: 200,
        }
    }
}

lazy_static! {
    pub static ref CONFIG: Arc<Config> = {
        let config_str = std::fs::read_to_string("config.toml")
            .expect("Failed to read config.toml");
        let config: Config = toml::from_str(&config_str)
            .expect("Failed to parse config.toml");

        Arc::new(config)
    };
}

pub fn get_config() -> Arc<Config> {
    Arc::clone(&CONFIG)
}

pub fn init_logging() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .init();
}

#[test]
fn test_config() {
    init_logging();
    let config = get_config();
    println!("{:?}", config);
}
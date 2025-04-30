use crate::error::BotError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub url: String,
    pub timeout_secs: u64,
    pub diy_mints_open: bool,
    pub diy_mints: Vec<String>,
    pub forbidden_mints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    pub max_loop: usize,
    pub compute_unit_limit: u32,
    pub batch_size: usize,
    pub max_concurrent_tasks: usize,
    pub debug: bool,
    pub auto_lookup_table: bool,
    pub mint_max_pool: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeUnitPrice {
    pub from: u64,
    pub to: u64,
    pub count: u64,
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpamConfig {
    pub compute_unit_price: ComputeUnitPrice,
    pub min_profit: u64,
    pub process_delay: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    pub keypair_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    pub urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KaminoFlashloan {
    pub enabled: bool,
    pub max_sol_amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api: ApiConfig,
    pub bot: BotConfig,
    pub spam: SpamConfig,
    pub wallet: WalletConfig,
    pub rpc: RpcConfig,
    pub mint_config_type: String,
    pub use_price_check: bool,
    pub use_simulation: bool,
    pub kamino_flashloan: KaminoFlashloan,
}

lazy_static::lazy_static! {
    static ref CONFIG: Arc<Config> = Arc::new(Config::parse().expect("Failed to load config"));
}

impl Config {
    /// 获取 API URL
    pub fn get_api_url(&self) -> &str {
        &self.api.url
    }

    /// 解析配置文件
    pub fn parse() -> Result<Self, BotError> {
        let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "wali_config.toml".to_string());

        // 检查配置文件是否存在
        if !Path::new(&config_path).exists() {
            return Err(BotError::Config(format!(
                "Configuration file not found at path: {}",
                config_path
            )));
        }

        // 读取并解析 TOML 文件
        let config_str = fs::read_to_string(&config_path)
            .map_err(|e| BotError::Config(format!("Failed to read config file: {}", e)))?;
        let config: Config = toml::from_str(&config_str)
            .map_err(|e| BotError::Config(format!("Failed to parse TOML config: {}", e)))?;
        Ok(config)
    }

    /// 获取全局配置
    pub fn get() -> Arc<Config> {
        Arc::clone(&CONFIG)
    }

    /// 返回密钥对文件路径
    pub fn get_keypair_path(&self) -> Result<&Path, BotError> {
        let path = Path::new(&self.wallet.keypair_path);
        if !path.exists() {
            return Err(BotError::Config(format!(
                "Keypair file not found at path: {}",
                self.wallet.keypair_path
            )));
        }
        Ok(path)
    }

    /// 加载密钥对文件
    pub fn load_keypair(&self) -> Result<String, BotError> {
        let path = Path::new(&self.wallet.keypair_path);
        if !path.exists() {
            return Err(BotError::Config(format!(
                "Keypair file not found at path: {}",
                self.wallet.keypair_path
            )));
        }

        let keypair_data = fs::read_to_string(path)
            .map_err(|e| BotError::Config(format!("Failed to read keypair file: {}", e)))?;
        Ok(keypair_data)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            api: ApiConfig {
                url: "http://64.130.33.49:11000/token/hot".to_string(),
                timeout_secs: 10,
                diy_mints_open: false,
                diy_mints: vec![],
                forbidden_mints: vec![],
            },
            bot: BotConfig {
                max_loop: 1000,
                compute_unit_limit: 300_000,
                batch_size: 1,
                max_concurrent_tasks: 50,
                debug: false,
                auto_lookup_table:false,
                mint_max_pool:4,
            },
            spam: SpamConfig {
                compute_unit_price: ComputeUnitPrice {
                    from: 2000,
                    to: 5000,
                    count: 3,
                    strategy: "random".to_string(),
                },
                min_profit: 1,
                process_delay: 400,
            },
            kamino_flashloan: KaminoFlashloan {
                enabled: false,
                max_sol_amount: 100_000_000_000,
            },
            wallet: WalletConfig {
                keypair_path: "my_keypair.json".to_string(),
            },
            rpc: RpcConfig {
                urls: vec!["https://api.mainnet-beta.solana.com".to_string()],
            },
            mint_config_type: "url".to_string(),
            use_price_check: false,
            use_simulation: false,
        }
    }
}
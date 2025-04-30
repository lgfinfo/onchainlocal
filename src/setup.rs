use solana_sdk::{
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair,Signer},
};
use spl_associated_token_account::get_associated_token_address;
use std::sync::{Arc,Mutex};
use std::str::FromStr;

use crate::{
    config::{self, MintConfig},
    wali_config::Config as ConfigJson,
    constants,
    error::BotError,
    rpc::RpcManager,
    shared_context::Context,
};
use log::info;
use std::collections::HashMap;
/// 初始化共享上下文
pub async fn initialize_context() -> Result<Arc<Context>, BotError> {
    let config = config::get_config();
    let rpc =RpcManager::new(config.clone());
    let wali_config = ConfigJson::get();
    let keypair_path = wali_config.get_keypair_path()?;

    let payer = read_keypair_file(keypair_path)
        .map_err(|e| BotError::Config(format!("读取密钥对失败: {}", e)))?;
    let payer_rc = Arc::new(payer);
    info!("钱包地址: {}", payer_rc.pubkey());
    let sol_mint = Pubkey::from_str(constants::SOL_MINT)
        .map_err(|e| BotError::Config(format!("无效 SOL mint: {}", e)))?;
    let wallet_sol_account = get_associated_token_address(&payer_rc.pubkey(), &sol_mint);
    Ok(Arc::new(Context::new(
        config,
        wali_config,
        wallet_sol_account,
        payer_rc,
        rpc,
    )))
}

/// 加载代币列表
pub async fn load_mint_list(context: &Context) -> Result<Vec<MintConfig>, BotError> {
    match context.wali_config.mint_config_type.as_str() {
        "url" => crate::api_config::get_api_config(context)
            .await
            .map_err(|e| BotError::Rpc(format!("获取代币列表失败: {}", e))),
        "config" => Ok(context.config.routing.mint_config_list.clone()),
        "json" => {
            info!("使用 JSON 文件加载代币列表");
            let pool_data_file = "pool_data_output.json";
            let file_content = std::fs::read_to_string(pool_data_file)?;
            let mint_configs: Vec<MintConfig> =serde_json::from_str(&file_content)
                .map_err(|e| BotError::Config(format!("解析 JSON 失败: {}", e)))?;
          
            Ok(mint_configs)
        },
        _ => Err(BotError::Config(
            "未知的 mint_config_type, 只支持 url/config/json".to_string(),
        )),
    }
}
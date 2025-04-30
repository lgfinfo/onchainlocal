use solana_sdk::{
    pubkey::Pubkey,
    signature::Keypair,
};
use std::time::Instant;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use crate::{
    config::Config,
    wali_config::Config as ConfigJson,
    rpc::RpcManager,
};

/// 共享上下文结构体，封装所有需要共享的资源
#[derive(Clone)]
pub struct Context {
    pub config: Arc<Config>,
    pub wali_config: Arc<ConfigJson>,
    pub wallet_sol_account: Pubkey,
    pub payer_rc: Arc<Keypair>,
    pub rpc: RpcManager,
}

impl Context {
    /// 创建新的 Context 实例
    pub fn new(
        config: Arc<Config>,
        wali_config: Arc<ConfigJson>,
        wallet_sol_account: Pubkey,
        payer_rc: Arc<Keypair>,
        rpc: RpcManager,
    ) -> Self {
        Self {
            config,
            wali_config,
            wallet_sol_account,
            payer_rc,
            rpc        
        }
    }
}
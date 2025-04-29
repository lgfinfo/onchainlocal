// 文件: src/generate.rs
// ```rust
use crate::refresh::initialize_pool_data;
use anyhow::Result;
use solana_program::instruction::{AccountMeta, Instruction};
use solana_program::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use std::str::FromStr;
use crate::error::BotError;
use crate::pools::MintPoolData;
use crate::shared_context::Context;
use solana_program::system_program;

use crate::constants::sol_mint;
use crate::dex::dlmm::constants::{dlmm_event_authority, dlmm_program_id};
use crate::dex::pump::constants::{pump_fee_wallet, pump_program_id};
use crate::dex::raydium::constants::{
    raydium_clmm_program_id, raydium_cp_program_id, raydium_program_id,
};
use crate::dex::whirlpool::constants::whirlpool_program_id;
use crate::dex::raydium::{raydium_authority, raydium_cp_authority};
use crate::config::MintConfig;
use spl_associated_token_account::ID as associated_token_program_id;
use spl_token::ID as token_program_id;

use log::info;

use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    address_lookup_table::state::AddressLookupTable,
};

pub fn generate_onchain_swap_multiple_mints_instruction_v4(
    context: &Context,
    token_pools: &[&MintPoolData]
) -> Instruction {

    let wallet=    &context.payer_rc.pubkey();
    let wallet_sol_account=    &context.wallet_sol_account;
    let compute_unit_limit = context.wali_config.bot.compute_unit_limit;
    let max_bin_to_process = ((compute_unit_limit
        - 200_000
        - if context.wali_config.kamino_flashloan.enabled { 80_000 } else { 0 }) / 10_000) as u64;
    let minimum_profit = context.wali_config.spam.min_profit;
    let batch_size= context.wali_config.bot.batch_size;

    info!("Creating swap instruction for all DEX types");

    let executor_program_id =
        Pubkey::from_str("MEViEnscUm6tsQRoGd9h6nLQaQspKj7DB2M5FwM3Xvz").unwrap();
    let fee_collector = Pubkey::from_str("6AGB9kqgSp2mQXwYpdrV4QVV8urvCaDS35U1wsLssy6H").unwrap();

    let pump_global_config =
        Pubkey::from_str("ADyA8hdefvWN2dbGGWFotbzWxrAvLW83WG6QCVXvJKqw").unwrap();
    let pump_authority = Pubkey::from_str("GS4CU59F31iL7aR2Q8zVS8DRrcRnXX1yjQ66TqNVQnaR").unwrap();

    let sol_mint_pubkey = sol_mint();

    let mut accounts = vec![
        AccountMeta::new_readonly(*wallet, true), // 0. Wallet (signer)
        AccountMeta::new_readonly(sol_mint_pubkey, false), // 1. SOL mint
        AccountMeta::new(fee_collector, false),  // 2. Fee collector
        AccountMeta::new(*wallet_sol_account, false), // 3. Wallet SOL account
        AccountMeta::new_readonly(token_program_id, false), // 4. Token program
        AccountMeta::new_readonly(system_program::ID, false), // 5. System program
        AccountMeta::new_readonly(associated_token_program_id, false), // 6. Associated Token program
    ];
    for mint_pool_data in token_pools {
        let mut take_num=2;
        if batch_size>1 {
            take_num = 1;
            if mint_pool_data.pump_pools.len()< 1||mint_pool_data.dlmm_pairs.len()< 1 {
                continue;
            }
        }

        accounts.push(AccountMeta::new_readonly(mint_pool_data.mint, false));
        let wallet_x_account =
            spl_associated_token_account::get_associated_token_address(&wallet, &mint_pool_data.mint);
        accounts.push(AccountMeta::new(wallet_x_account, false));
     
        for pool in mint_pool_data.pump_pools.iter().take(take_num) {
            accounts.push(AccountMeta::new_readonly(pump_program_id(), false));
            accounts.push(AccountMeta::new_readonly(pump_global_config, false));
            accounts.push(AccountMeta::new_readonly(pump_authority, false));
            accounts.push(AccountMeta::new_readonly(pump_fee_wallet(), false));
            accounts.push(AccountMeta::new_readonly(pool.pool, false));
            accounts.push(AccountMeta::new(pool.token_vault, false));
            accounts.push(AccountMeta::new(pool.sol_vault, false));
            accounts.push(AccountMeta::new(pool.fee_token_wallet, false));
        }

        for pair in mint_pool_data.dlmm_pairs.iter().take(take_num) {
            accounts.push(AccountMeta::new_readonly(dlmm_program_id(), false));
            accounts.push(AccountMeta::new(dlmm_event_authority(), false)); // DLMM event authority
            accounts.push(AccountMeta::new(pair.pair, false));
            accounts.push(AccountMeta::new(pair.token_vault, false));
            accounts.push(AccountMeta::new(pair.sol_vault, false));
            accounts.push(AccountMeta::new(pair.oracle, false));
            for bin_array in &pair.bin_arrays {
                accounts.push(AccountMeta::new(*bin_array, false));
            }
        }
        if batch_size>1{
            continue;
        }
        for pool in mint_pool_data.raydium_pools.iter().take(take_num) {
            accounts.push(AccountMeta::new_readonly(raydium_program_id(), false));
            accounts.push(AccountMeta::new_readonly(raydium_authority(), false)); // Raydium authority
            accounts.push(AccountMeta::new(pool.pool, false));
            accounts.push(AccountMeta::new(pool.token_vault, false));
            accounts.push(AccountMeta::new(pool.sol_vault, false));
        }

        for pool in mint_pool_data.raydium_cp_pools.iter().take(take_num) {
            accounts.push(AccountMeta::new_readonly(raydium_cp_program_id(), false));
            accounts.push(AccountMeta::new_readonly(raydium_cp_authority(), false)); // Raydium CP authority
            accounts.push(AccountMeta::new(pool.pool, false));
            accounts.push(AccountMeta::new_readonly(pool.amm_config, false));
            accounts.push(AccountMeta::new(pool.token_vault, false));
            accounts.push(AccountMeta::new(pool.sol_vault, false));
            accounts.push(AccountMeta::new(pool.observation, false));
        }

        for pool in mint_pool_data.whirlpool_pools.iter().take(take_num) {
            accounts.push(AccountMeta::new_readonly(whirlpool_program_id(), false));
            accounts.push(AccountMeta::new(pool.pool, false));
            accounts.push(AccountMeta::new(pool.oracle, false));
            accounts.push(AccountMeta::new(pool.x_vault, false));
            accounts.push(AccountMeta::new(pool.y_vault, false));
            for tick_array in &pool.tick_arrays {
                accounts.push(AccountMeta::new(*tick_array, false));
            }
        }

        for pool in mint_pool_data.raydium_clmm_pools.iter().take(take_num) {
            accounts.push(AccountMeta::new_readonly(raydium_clmm_program_id(), false));
            accounts.push(AccountMeta::new(pool.pool, false));
            accounts.push(AccountMeta::new_readonly(pool.amm_config, false));
            accounts.push(AccountMeta::new(pool.observation_state, false));
            accounts.push(AccountMeta::new(pool.x_vault, false));
            accounts.push(AccountMeta::new(pool.y_vault, false));
            for tick_array in &pool.tick_arrays {
                accounts.push(AccountMeta::new(*tick_array, false));
            }
        }
    }
    info!("   Total accounts: {}", accounts.len());
    let mut data = vec![14u8];

    let minimum_profit: u64 = minimum_profit;
    let max_bin_to_process: u64 = max_bin_to_process;

    data.extend_from_slice(&minimum_profit.to_le_bytes());
    data.extend_from_slice(&max_bin_to_process.to_le_bytes());

    Instruction {
        program_id: executor_program_id,
        accounts,
        data,
    }
}

pub async fn pull_token_pools(
    context: &Context,
    mint_config_list: &Vec<MintConfig>
) -> Result<Vec<(String,MintPoolData, Vec<AddressLookupTableAccount>)>, BotError> {
    let mut token_pools: Vec<(String,MintPoolData, Vec<AddressLookupTableAccount>)> = vec![];

    for x_mint in mint_config_list {            
        let pool_data = initialize_pool_data(
            &x_mint.mint,
            &context.payer_rc.pubkey().to_string(),
            &x_mint.raydium_pool_list,
            &x_mint.raydium_cp_pool_list,
            &x_mint.pump_pool_list,
            &x_mint.meteora_dlmm_pool_list,
            &x_mint.whirlpool_pool_list,
            &x_mint.raydium_clmm_pool_list,
            context.rpc.clone(),
        ).await.map_err(|e| BotError::Rpc(format!("获取池数据失败: {}", e)))?;
        let mut lookup_table_accounts = x_mint.lookup_table_accounts.clone();
        lookup_table_accounts.push("4sKLJ1Qoudh8PJyqBeuKocYdsZvxTcRShUt9aKqwhgvC".to_string());

        let mut lookup_table_accounts_list = vec![];

        for lookup_table_account in lookup_table_accounts {
            match Pubkey::from_str(&lookup_table_account) {
                Ok(pubkey) => {
                    match context.rpc.get_account(pubkey).await {
                        Ok(account) => {
                            match AddressLookupTable::deserialize(&account.data) {
                                Ok(lookup_table) => {
                                    let lookup_table_account = AddressLookupTableAccount {
                                        key: pubkey,
                                        addresses: lookup_table.addresses.into_owned(),
                                    };
                                    lookup_table_accounts_list.push(lookup_table_account);
                                    info!("   Successfully loaded lookup table: {}", pubkey);
                                    info!("");
                                }
                                Err(e) => {
                                    info!(
                                        "   Failed to deserialize lookup table {}: {}",
                                        pubkey, e
                                    );
                                    continue; // Skip this lookup table but continue processing others
                                }
                            }
                        }
                        Err(e) => {
                            info!("   Failed to fetch lookup table account {}: {}", pubkey, e);
                            continue; // Skip this lookup table but continue processing others
                        }
                    }
                }
                Err(e) => {
                    info!(
                        "   Invalid lookup table pubkey string {}: {}",
                        lookup_table_account, e
                    );
                    continue; // Skip this lookup table but continue processing others
                }
            }
        }
        if lookup_table_accounts_list.is_empty() {
            info!("   Warning: No valid lookup tables were loaded");
        } else {
            info!(
                "   Loaded {} lookup tables successfully",
                lookup_table_accounts_list.len()
            );
        }
        token_pools.push((x_mint.mint.to_string(),pool_data, lookup_table_accounts_list));
    }
    Ok(token_pools)
}
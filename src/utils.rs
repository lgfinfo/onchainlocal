// 文件: src/utils.rs
// ```rust
use crate::config;
use crate::error::BotError;
use crate::rpc::RpcManager;
use log::info;
use solana_program::{
    instruction::Instruction, message::VersionedMessage, message::v0::Message as MessageV0,
};
use solana_sdk::{
    pubkey,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::VersionedTransaction,
};
use spl_associated_token_account::get_associated_token_address;
use std::sync::Arc; // 修复：导入 Signer trait

use crate::kamino::{get_kamino_flashloan_borrow_ix, get_kamino_flashloan_repay_ix};
use crate::shared_context::Context;
use rand::{Rng, thread_rng};
use spl_token::id as spl_token_program_id;
use spl_token_2022::id as token_2022_program_id;
pub async fn calculate_dynamic_priority_fee(
    rpc: &RpcManager,
    addresses: &[Pubkey],
    num_fees: usize,
    max_fee_limit: u64,
    min_fee_limit: u64,
) -> Result<Vec<u64>, BotError> {
    let owned_addresses = addresses.to_vec();

    use rand::{Rng, thread_rng};

    let prioritization_fees = rpc
        .call(move |client| {
            let fees = client
                .get_recent_prioritization_fees(&owned_addresses)
                .map_err(|e| BotError::Instruction(e.to_string()));
            match fees {
                Ok(fees) => {
                    let prioritization_fees: Vec<u64> =
                        fees.into_iter().map(|fee| fee.prioritization_fee).collect();
                    Ok(prioritization_fees) // 直接返回 Vec<u64>
                }
                Err(e) => Err(BotError::Instruction(e.to_string())),
            }
        })
        .await
        .or_else(|_| {
            // 验证费用范围
            if min_fee_limit > max_fee_limit {
                return Err(BotError::Instruction(
                    "min_fee_limit 必须小于或等于 max_fee_limit".into(),
                ));
            }
            let mut rng = thread_rng();
            let random_fees: Vec<u64> = (0..50)
                .map(|_| rng.gen_range(min_fee_limit..=max_fee_limit))
                .collect();
            Ok(random_fees) // 直接返回 Vec<u64>
        })?;

    let mut prioritization_fees = prioritization_fees; // 已经解包为 Vec<u64>
    prioritization_fees.sort();

    let index = (prioritization_fees.len() as f64 * 0.9).floor() as usize;
    let mut percentile_90 = prioritization_fees[index.min(prioritization_fees.len() - 1)];

    if percentile_90 < min_fee_limit {
        percentile_90 = min_fee_limit;
    }

    if percentile_90 > max_fee_limit {
        percentile_90 = max_fee_limit;
    }

    let max_fee = (percentile_90 as f64 * 1.15) as u64;
    let min_fee = (percentile_90 as f64 * 0.85) as u64;

    let mut rng = thread_rng();
    let random_fees: Vec<u64> = (0..num_fees)
        .map(|_| rng.gen_range(min_fee..=max_fee))
        .collect();
    info!(
        "Returning{}  {} random fees: {:?}",
        addresses[0].to_string(),
        num_fees,
        random_fees
    );
    Ok(random_fees)
}

pub async fn get_or_create_ata_address(
    token_mint: Pubkey,
    payer_rc: &Arc<Keypair>,
) -> Result<Pubkey, BotError> {
    let config = config::get_config();
    let rpc_manager = RpcManager::new(config.clone());

    // 步骤 1：查询 Mint 账户以确定其 Token Program
    let mint_account = rpc_manager
        .get_account(token_mint)
        .await
        .map_err(|_| BotError::InvalidTokenProgram("无法获取 Mint 账户".to_string()))?;

    let token_program_id = mint_account.owner;
    // 步骤 2：验证 Token Program
    if token_program_id != spl_token_program_id() && token_program_id != token_2022_program_id() {
        return Err(BotError::InvalidTokenProgram(format!(
            "代币 Mint {} 的 Token Program 不支持: {}",
            token_mint, token_program_id
        )));
    }

    // 步骤 3：计算 ATA 地址
    let ata_address = get_associated_token_address(&payer_rc.pubkey(), &token_mint);

    // 步骤 4：检查 ATA 是否存在
    match rpc_manager.get_account(ata_address).await {
        Ok(account) => {
            // 验证 ATA 的 owner 是否正确
            if account.owner != token_program_id {
                return Err(BotError::InvalidTokenProgram(format!(
                    "ATA {} 已存在，但其 owner 不正确: {}",
                    ata_address, account.owner
                )));
            }
            info!("代币 Mint {} 的 ATA 已存在: {}", token_mint, ata_address);
        }
        Err(_) => {
            // 步骤 5：创建 ATA
            let instruction =
                spl_associated_token_account::instruction::create_associated_token_account(
                    &payer_rc.pubkey(), // 付款人
                    &payer_rc.pubkey(), // ATA 持有者
                    &token_mint,        // 代币 Mint
                    &token_program_id,  // Token Program ID
                );
            info!(
                "正在为代币 Mint {} 创建 ATA: 付款人 {}",
                token_mint,
                &payer_rc.pubkey()
            );

            let blockhash = rpc_manager
                .get_latest_blockhash()
                .await
                .map_err(|e| BotError::Rpc(e.to_string()))?;
            let message =
                MessageV0::try_compile(&payer_rc.pubkey(), &[instruction], &[], blockhash)
                    .map_err(|e| BotError::Instruction(format!("无法编译消息: {}", e)))?;
            let tx = VersionedTransaction::try_new(VersionedMessage::V0(message), &[&payer_rc])
                .map_err(|e| BotError::Instruction(format!("无法创建交易: {}", e)))?;

            rpc_manager
                .send_and_confirm_transaction(tx)
                .await
                .map_err(|e| {
                    println!("无法创建ata: {}", token_mint);
                    BotError::Rpc(format!("交易失败: {}", e))
            })?;
        }
    }

    Ok(ata_address)
}

pub async fn extend_loan_start(instructions: &mut Vec<Instruction>, context: &Context) {
    let borrow_ix = get_kamino_flashloan_borrow_ix(
        &context.payer_rc.pubkey(),
        get_associated_token_address(
            &context.payer_rc.pubkey(),
            &pubkey!("So11111111111111111111111111111111111111112"),
        ),
        context.wali_config.kamino_flashloan.max_sol_amount,
    )
    .unwrap();
    instructions.push(borrow_ix);
}

pub async fn extend_loan_end(instructions: &mut Vec<Instruction>, context: &Context) {
    let repay_ix = get_kamino_flashloan_repay_ix(
        &context.payer_rc.pubkey(),
        get_associated_token_address(
            &context.payer_rc.pubkey(),
            &pubkey!("So11111111111111111111111111111111111111112"),
        ),
        2, // Borrow instruction index
        context.wali_config.kamino_flashloan.max_sol_amount,
    )
    .unwrap();
    instructions.push(repay_ix);
}

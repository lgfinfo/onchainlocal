use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    address_lookup_table::state::AddressLookupTable, 
    compute_budget::ComputeBudgetInstruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signature::Keypair,
    signature::Signer,
    transaction::VersionedTransaction,
    commitment_config::CommitmentConfig,
};
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_program::instruction::Instruction;
use tokio::sync::{mpsc, Semaphore};
use tokio::task;
use futures::future::join_all;
use log::{info, warn};
use std::{sync::Arc, time::Duration};
use std::io::Write;
use std::collections::HashSet;
use std::fs::OpenOptions;
use crate::{
    error::BotError,
    utils::{calculate_dynamic_priority_fee,extend_loan_end,extend_loan_start},
    generate::generate_onchain_swap_multiple_mints_instruction_v4,
    shared_context::Context
};
use solana_program::address_lookup_table::instruction::{create_lookup_table, extend_lookup_table};
use crate::pools::MintPoolData;
use std::time::Instant;
/// 处理代币池，生成指令并执行交易
pub async fn run_bot(
    context: &Arc<Context>,
    token_pools: &Vec<(String,MintPoolData,Vec<AddressLookupTableAccount>)>
) -> Result<(), BotError> {
    let semaphore=Arc::new(Semaphore::new(context.wali_config.bot.max_concurrent_tasks));
    let (tx, mut rx) = mpsc::channel(token_pools.len()); // 用于收集指令组
    let mut handles = vec![];

    // 并行生成指令
    for chunk in token_pools.chunks(context.wali_config.bot.batch_size) {
        let _permit = semaphore.clone().acquire_owned().await
            .map_err(|e| BotError::Concurrency(format!("获取信号量失败: {}", e)))?;
        let chunk = chunk.to_vec();
        let context = context.clone(); // 克隆 Arc<Context>
        let tx = tx.clone();

        let handle = task::spawn(async move {
            let mut instruction_groups = vec![];

            let instructions = generate_instructions(&chunk.iter().map(|(_,mint_pool_data, _)| mint_pool_data).collect::<Vec<_>>(), &context).await?;
            let mut lookup_table_accounts_list:Vec<AddressLookupTableAccount> = vec![];
            let mut group_instruction_key=String::new();
            for (i,(mint_pubkey,__mint_pool_data, address_lookup_table)) in chunk.iter().enumerate() {   
                group_instruction_key.push_str(mint_pubkey.as_str());
                if i < chunk.len() - 1 {
                    group_instruction_key.push_str("|"); // 添加分割符
                }
                lookup_table_accounts_list.extend(address_lookup_table.iter().cloned());
            }
            deduplicate_and_update_lookup_table(&mut lookup_table_accounts_list, &instructions.clone(), "lookup_table.csv",&context).await?;

            instruction_groups.push((group_instruction_key, instructions, lookup_table_accounts_list));
            tx.send(instruction_groups).await
                .map_err(|e| BotError::Concurrency(format!("发送指令组失败: {}", e)))?;
            Ok::<_, BotError>(())
        });
        handles.push(handle);
    }

    // 关闭通道的一部分发送端
    drop(tx);

    // 收集所有指令组
    let mut all_instruction_groups = vec![];
    while let Some(instruction_groups) = rx.recv().await {
        all_instruction_groups.extend(instruction_groups);
    }

    // 等待所有任务完成并检查错误
    for (i, handle) in join_all(handles).await.into_iter().enumerate() {
        match handle {
            Ok(Ok(())) => info!("批次 #{} 处理成功", i + 1),
            Ok(Err(e)) => warn!("批次 #{} 处理失败: {}", i + 1, e),
            Err(e) => warn!("任务 #{} 异常: {:?}", i + 1, e),
        }
    }

    // 配置交易发送参数
    let tx_config = RpcSendTransactionConfig {
        skip_preflight: true,
        preflight_commitment: Some(CommitmentConfig::confirmed().commitment),
        encoding: None,
        max_retries: Some(3),
        min_context_slot: None,
    };


    // 处理交易
    let mut i = 0;
    loop {
        i += 1;
        info!("捕获的 instruction_groups 总数: {}", all_instruction_groups.len());
        for (mint, instructions, lookup_table_accounts) in &all_instruction_groups {
            info!("处理 mint: {}", mint);
            match optimize_and_build_transaction_with_alt(&context,&mint, instructions, &lookup_table_accounts).await {
                Ok(tx) => {
                    let serialized = bincode::serialize(&tx).expect("序列化交易失败");
                    info!("交易大小: {} 字节", serialized.len());
                    if serialized.len() <= 10{
                        if context.wali_config.use_simulation {
                            let tx_clone = tx.clone();
                            let sim_result = context.rpc.call(move |client| 
                                client.simulate_transaction(&tx_clone).map_err(|e| BotError::Instruction(e.to_string())));
                            match sim_result.await {
                                Ok(response) => info!("模拟结果: {:?}", response),
                                Err(e) => warn!("模拟失败: {:?}", e),
                            }
                        }
                        let mut attempts=0;
                        while attempts<context.wali_config.spam.compute_unit_price.count{
                            attempts+=1;
                            match context.rpc.send_transaction_with_config(tx.clone(), tx_config.clone()).await {
                                Ok(tx_signature) => {
                                    info!("交易成功: mint={}, signature={}", mint, tx_signature);
                                    if context.wali_config.spam.process_delay>0 {
                                        // 处理延迟
                                        info!("等待 {} 毫秒", context.wali_config.spam.process_delay);
                                        tokio::time::sleep(tokio::time::Duration::from_millis(context.wali_config.spam.process_delay)).await;
                                    }
                                }
                                Err(e) => {
                                    warn!("交易失败: mint={}, error={}", mint, e);
                                    // tokio::time::sleep(tokio::time::Duration::from_millis(context.)).await;
                                }
                            }
                            
                        }
                    }
                }
                Err(e) => {
                    warn!("构建交易失败: mint={}, error={}", mint, e.to_string());
                }
            }
        }
        
        if i > context.wali_config.bot.max_loop&&context.wali_config.bot.max_loop!=0 {
            break;
        }
    }

    Ok(())
}

/// 为分组代币池生成指令
pub async fn generate_instructions(
    token_pool: &[&MintPoolData],
    context: &Context,
) -> Result<Vec<Instruction>, BotError> {
    let mut instructions = vec![];

    let compute_unit_limit = context.wali_config.bot.compute_unit_limit;

    let compute_unit_price = calculate_dynamic_priority_fee(
        &context.rpc,
        &[token_pool[0].mint],
        context.wali_config.bot.batch_size,
        context.wali_config.spam.compute_unit_price.to,
        context.wali_config.spam.compute_unit_price.from,
    )
    .await
    .map_err(|e| BotError::Rpc(format!("计算优先级费用失败: {}", e)))?;

    instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(compute_unit_limit));
    instructions.push(ComputeBudgetInstruction::set_compute_unit_price(compute_unit_price[0]));
    if context.wali_config.kamino_flashloan.enabled {
        extend_loan_start(&mut instructions,&context).await;
    }

    let instruction = generate_onchain_swap_multiple_mints_instruction_v4(
        &context,
        token_pool,
    );
    instructions.push(instruction);
    if context.wali_config.kamino_flashloan.enabled {
        extend_loan_end(&mut instructions,&context).await;
    }
    Ok(instructions)
}

/// 优化并构建交易，使用地址查找表（ALT）
pub async fn optimize_and_build_transaction_with_alt(
    context: &Context,
    mint:&String,
    instructions: &[Instruction],
    address_lookup_table_accounts: &Vec<AddressLookupTableAccount>,
) -> Result<VersionedTransaction, BotError> {
    info!("开始构建交易...");
    let price= get_cached_price(context, mint);
    info!("当前价格:{}",price);
    // 创建交易
    let blockhash = context.rpc.get_latest_blockhash()
        .await
        .map_err(|e| BotError::Rpc(format!("获取 blockhash 失败: {}", e)))?;
    let message = Message::try_compile(&context.payer_rc.pubkey(), instructions, address_lookup_table_accounts, blockhash)
        .map_err(|e| BotError::Transaction(format!("编译交易消息失败: {}", e)))?;
    let versioned_message = VersionedMessage::V0(message);
    let tx = VersionedTransaction::try_new(versioned_message, &[&context.payer_rc])
        .map_err(|e| BotError::Transaction(format!("创建交易失败: {}", e)))?;
    debug_transaction(&tx);
    Ok(tx)
}

/// 调试交易信息
pub fn debug_transaction(tx: &VersionedTransaction) {
    let serialized = bincode::serialize(tx).expect("序列化交易失败");
    info!("交易大小: {} 字节", serialized.len());
    if let solana_sdk::message::VersionedMessage::V0(ref message) = tx.message {
        for (i, instruction) in message.instructions.iter().enumerate() {
            info!("指令 {}: 账户数={}, 数据长度={}", i, instruction.accounts.len(), instruction.data.len());
        }
    }
    info!("签名数: {}", tx.signatures.len());
}

/// 去重并更新查找表
pub async fn deduplicate_and_update_lookup_table(
    lookup_table_accounts_list: &mut Vec<AddressLookupTableAccount>,
    instructions: &[Instruction],
    csv_file_path: &str,
    context: &Context,
) -> Result<(), BotError> {
    // 提取所有地址
    let mut table_addresses: HashSet<Pubkey> = HashSet::new();

    // 从现有的查找表中提取地址
    for lookup_table in lookup_table_accounts_list.iter() {
        table_addresses.extend(lookup_table.addresses.iter().cloned());
    }

    let mut instruction_addresses: HashSet<Pubkey> = HashSet::new();
    // 从指令中提取账户地址
    for instruction in instructions.iter() {
        instruction_addresses.extend(instruction.accounts.iter().map(|account_meta| account_meta.pubkey));
    }

    // 去重后的地址列表
    // 从 instruction_addresses 中剔除 table_addresses 中的地址
    let unique_addresses: HashSet<Pubkey> = instruction_addresses
     .difference(&table_addresses)
     .cloned()
     .collect();
  
    if unique_addresses.is_empty() {
        info!("没有新的地址需要添加到查找表中");
        return Ok(());
    }
    if !context.wali_config.bot.auto_lookup_table{
        info!("有新的地址需要添加到查找表中{:#?},但您没有开启生成查找表，可能造成指令无法发出",unique_addresses);

        return Ok(());
    }

    info!("有新的地址需要添加到查找表中{:#?}",unique_addresses);
    // 创建新的查找表
    let (create_lookup_table_ix, new_lookup_table_key) = create_lookup_table(
        context.payer_rc.pubkey(),       // 查找表创建者的公钥
        context.payer_rc.pubkey(),       // 付款人公钥
        context.rpc.get_slot().await?, // 当前 slot
    );
    save_lookup_table_to_csv(&new_lookup_table_key, csv_file_path)?;
    // 更新查找表列表
    let blockhash = context.rpc.get_latest_blockhash().await.map_err(|e| BotError::Rpc(format!("获取 blockhash 失败: {}", e)))?;
    let message = Message::try_compile(&context.payer_rc.pubkey(), &[create_lookup_table_ix], &[], blockhash)
        .map_err(|e| BotError::Transaction(format!("编译交易消息失败: {}", e)))?;
    let versioned_message = VersionedMessage::V0(message);
    let tx = VersionedTransaction::try_new(versioned_message, &[&context.payer_rc])
        .map_err(|e| BotError::Transaction(format!("创建交易失败: {}", e)))?;
   
    let _sign=context.rpc.send_and_confirm_transaction(tx).await.map_err(|e| BotError::Transaction(format!("发送交易失败: {}", e)))?;
    
    let extend_ix = extend_lookup_table(
        new_lookup_table_key, // 查找表地址
        context.payer_rc.pubkey(),        // 权限账户
        Some(context.payer_rc.pubkey()),  // 付款人
        unique_addresses.clone().into_iter().collect::<Vec<Pubkey>>(),    // 要添加的地址列表
    );
    let ex_blockhash = context.rpc.get_latest_blockhash().await.map_err(|e| BotError::Rpc(format!("获取 blockhash 失败: {}", e)))?;
    let ex_message = Message::try_compile(&context.payer_rc.pubkey(), &[extend_ix], &[], ex_blockhash)
    .map_err(|e| BotError::Transaction(format!("编译交易消息失败: {}", e)))?;
    let ex_versioned_message = VersionedMessage::V0(ex_message);
    let tx_extend = VersionedTransaction::try_new(ex_versioned_message, &[&context.payer_rc])
    .map_err(|e| BotError::Transaction(format!("创建交易失败: {}", e)))?;
    let _ex_sign=context.rpc.send_and_confirm_transaction(tx_extend).await.map_err(|e| BotError::Transaction(format!("发送交易失败: {}", e)))?;
    // 保存新的查找表地址到 CSV 文件
    lookup_table_accounts_list.push(AddressLookupTableAccount {
        key: new_lookup_table_key,
        addresses: unique_addresses.into_iter().collect::<Vec<Pubkey>>(),
    });

    Ok(())
}

/// 保存查找表地址到 CSV 文件
fn save_lookup_table_to_csv(lookup_table_key: &Pubkey, csv_file_path: &str) -> Result<(), BotError> {
    let mut file = OpenOptions::new()
    .create(true) // 如果文件不存在则创建
    .append(true) // 如果文件存在则追加
    .open(csv_file_path)
    .map_err(|e| BotError::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("打开 CSV 文件失败: {}", e))))?;

    // 如果文件是新创建的，写入表头
    if file.metadata().map_err(|e| BotError::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("获取文件元数据失败: {}", e))))?.len() == 0 {
        writeln!(file, "lookup_table_key")
            .map_err(|e| BotError::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("写入 CSV 文件失败: {}", e))))?;
    }

    // 写入新的查找表地址
    writeln!(file, "{}", lookup_table_key)
        .map_err(|e| BotError::Io(std::io::Error::new(std::io::ErrorKind::Other, format!("写入 CSV 文件失败: {}", e))))?;

    Ok(())
}

async fn compute_price_refresher(
    context: &Context,
    address_mint:Vec<String>,
) {
    let cache_ttl = Duration::from_secs(30);
    loop {
        for mint in address_mint.iter() {
            let cached = context.price_cache.lock().unwrap().get(mint)
                .filter(|(_, time)| time.elapsed() < cache_ttl);
            if cached.is_none() {
                let compute_unit_price = calculate_dynamic_priority_fee(
                    &context.rpc,
                    &[mint.parse().unwrap()],
                    context.wali_config.rpc.urls.len(),
                    context.wali_config.spam.compute_unit_price.to,
                    context.wali_config.spam.compute_unit_price.from,
                )
                .await
                .map_err(|e| BotError::Rpc(format!("计算优先级费用失败: {}:{}",mint, e)));

                match compute_unit_price {
                    Ok(price) => {
                        let mut cache = context.price_cache.write().unwrap();
                        cache.insert(mint.clone(), (price, Instant::now()));
                        info!("{} 更新缓存价格: {}", mint, price[0]);
                    },
                    Err(e) => {
                        warn!("价格更新失败: {}", e);
                        continue;
                    },
                    _ => continue,
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn get_cached_price(context: &Context, mint: &str) -> Option<Vec<u64>> {
    let cache_read  = context.price_cache.read().unwrap();
    cache_read.get(mint).map(|(price, _)| price.clone())
}
// async fn deactivate_lookup_table(
//     context: &Context,
//     payer: &Keypair,
//     lookup_table_address: &Pubkey,
// ) -> Result<()> {
//     // 查找表的权限账户（通常是创建者）
//     let authority = payer.pubkey();

//     // 构建停用指令
//     let instruction = Instruction {
//         program_id: solana_sdk::address_lookup_table::program::id(),
//         accounts: vec![
//             AccountMeta::new(*lookup_table_address, false),
//             AccountMeta::new_readonly(authority, true),
//         ],
//         data: solana_sdk::address_lookup_table::instruction::deactivate_lookup_table().data,
//     };

//     // 创建交易
//     let recent_blockhash = context.rpc.get_latest_blockhash().await?;
//     let message = Message::new(&[instruction], Some(&payer.pubkey()));
//     let mut transaction = Transaction::new(&message, vec![payer], recent_blockhash);

//     // 发送并确认交易
//     let signature = context.rpc.send_and_confirm_transaction(&transaction)?;
//     println!("Lookup table deactivated. Signature: {}", signature);

//     Ok(())
// }

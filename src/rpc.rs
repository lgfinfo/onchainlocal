use solana_client::{
    rpc_client::RpcClient,
    rpc_config::RpcSendTransactionConfig,
    rpc_response::RpcPrioritizationFee,
};
use solana_sdk::{
    account::Account,
    pubkey::Pubkey,
    signature::Signature,
    transaction::VersionedTransaction,
};
use tokio::{
    sync::mpsc::{self, Receiver, Sender},
    task::JoinHandle,
    time::{sleep, Duration, Instant},
};
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}, Mutex};
use log::{info, warn, error};
use rand::Rng;

use crate::{
    config::Config,
    error::BotError,
};

/// 管理 Solana RPC 客户端，异步处理链上请求。
pub struct RpcManager {
    sender: Arc<Sender<RpcRequest>>,
    handle: Arc<JoinHandle<()>>,
    process_delay: u64,
    clients_status: Arc<Mutex<Vec<ClientStatus>>>,
}

/// 客户端状态，记录健康状况和性能指标
#[derive(Clone)]
struct ClientStatus {
    is_healthy: bool,
    avg_response_time_ms: f64,
    success_count: usize,
    failure_count: usize,
    last_checked: Instant,
}

impl ClientStatus {
    fn new() -> Self {
        ClientStatus {
            is_healthy: true,
            avg_response_time_ms: 100.0,
            success_count: 0,
            failure_count: 0,
            last_checked: Instant::now(),
        }
    }

    fn update_response_time(&mut self, response_time_ms: f64) {
        let alpha = 0.2;
        self.avg_response_time_ms = alpha * response_time_ms + (1.0 - alpha) * self.avg_response_time_ms;
    }

    fn weight(&self) -> f64 {
        if !self.is_healthy { 0.0 } else { 1000.0 / (self.avg_response_time_ms + 1.0) }
    }
}

enum RpcRequest {
    GetLatestBlockhash(Sender<Result<solana_sdk::hash::Hash, BotError>>),
    SendTransaction(VersionedTransaction, Sender<Result<Signature, BotError>>),
    Call(
        Box<dyn Fn(&RpcClient) -> Result<Box<dyn std::any::Any + Send>, BotError> + Send + Sync>,
        Sender<Result<Box<dyn std::any::Any + Send>, BotError>>,
    ),
    GetAccount(Pubkey, Sender<Result<Account, BotError>>),
    GetSlot(Sender<Result<u64, BotError>>),
    SendAndConfirmTransaction(VersionedTransaction, Sender<Result<Signature, BotError>>),
    SendTransactionWithConfig(
        VersionedTransaction,
        RpcSendTransactionConfig,
        Sender<Result<Signature, BotError>>,
    ),
    ConfirmTransaction(Signature, Sender<Result<bool, BotError>>),
}

impl Clone for RpcManager {
    fn clone(&self) -> Self {
        RpcManager {
            sender: Arc::clone(&self.sender),
            handle: Arc::clone(&self.handle),
            process_delay: self.process_delay,
            clients_status: Arc::clone(&self.clients_status),
        }
    }
}

impl RpcManager {
    /// 创建新的 RPC 管理器
    pub fn new(config: Arc<Config>) -> Self {
        let wali_config = config.get_wali_config();
        let clients: Vec<Arc<RpcClient>> = wali_config
            .rpc
            .urls
            .iter()
            .map(|url| Arc::new(RpcClient::new_with_timeout(url.clone(), Duration::from_secs(30))))
            .collect();
        let clients = if clients.is_empty() {
            vec![Arc::new(RpcClient::new_with_timeout(
                config.rpc.url.clone(),
                Duration::from_secs(30),
            ))]
        } else {
            clients
        };
        let clients = Arc::new(clients);
        let clients_status = Arc::new(Mutex::new(vec![ClientStatus::new(); clients.len()]));

        let (sender, receiver) = mpsc::channel(200);
        let sender = Arc::new(sender);
        let client_index = Arc::new(AtomicUsize::new(0));
        let handle = Arc::new(tokio::spawn(run_request_loop(
            clients,
            receiver,
            client_index,
            config.clone(),
            clients_status.clone(),
        )));

        RpcManager {
            sender,
            handle,
            process_delay: wali_config.spam.process_delay,
            clients_status,
        }
    }

    /// 关闭 RPC 管理器，释放资源
    pub async fn shutdown(&self) {
        drop(self.sender.as_ref().clone());
        if let Ok(handle) = Arc::try_unwrap(self.handle.clone()) {
            handle.await.unwrap_or_else(|e| warn!("任务关闭失败: {:?}", e));
        }
    }

    /// 获取最新 blockhash
    pub async fn get_latest_blockhash(&self) -> Result<solana_sdk::hash::Hash, BotError> {
        let (tx, mut rx) = mpsc::channel(1);
        self.sender
            .send(RpcRequest::GetLatestBlockhash(tx))
            .await
            .map_err(|e| BotError::Rpc(format!("发送 blockhash 请求失败: {}", e)))?;
        rx.recv()
            .await
            .ok_or(BotError::Rpc("接收 blockhash 响应失败".into()))?
    }

    /// 发送交易
    pub async fn send_transaction(&self, tx: VersionedTransaction) -> Result<Signature, BotError> {
        let (tx_channel, mut rx) = mpsc::channel(1);
        self.sender
            .send(RpcRequest::SendTransaction(tx, tx_channel))
            .await
            .map_err(|e| BotError::Rpc(format!("发送交易请求失败: {}", e)))?;
        rx.recv()
            .await
            .ok_or(BotError::Rpc("接收交易响应失败".into()))?
    }

    /// 调用自定义 RPC 方法
    pub async fn call<F, T>(&self, f: F) -> Result<T, BotError>
    where
        F: Fn(&RpcClient) -> Result<T, BotError> + Send + Sync + 'static,
        T: Send + 'static,
    {
        let (tx, mut rx) = mpsc::channel(1);
        self.sender
            .send(RpcRequest::Call(
                Box::new(move |client: &RpcClient| {
                    Ok(Box::new(f(client)?) as Box<dyn std::any::Any + Send>)
                }),
                tx,
            ))
            .await
            .map_err(|e| BotError::Rpc(format!("发送 call 请求失败: {}", e)))?;
        let result = rx
            .recv()
            .await
            .ok_or(BotError::Rpc("接收 call 响应失败".into()))??;
        let result = result
            .downcast::<T>()
            .map_err(|_| BotError::Rpc("类型转换失败".into()))?;
        Ok(*result)
    }

    /// 获取账户信息
    pub async fn get_account(&self, pubkey: Pubkey) -> Result<Account, BotError> {
        let (tx, mut rx) = mpsc::channel(1);
        self.sender
            .send(RpcRequest::GetAccount(pubkey, tx))
            .await
            .map_err(|e| BotError::Rpc(format!("发送账户请求失败: {}", e)))?;
        rx.recv()
            .await
            .ok_or(BotError::Rpc("接收账户响应失败".into()))?
    }

    /// 批量获取账户信息
    pub async fn get_accounts(&self, pubkeys: Vec<Pubkey>) -> Result<Vec<Account>, BotError> {
        let (tx, mut rx) = mpsc::channel(pubkeys.len());
        for pubkey in pubkeys {
            self.sender
                .send(RpcRequest::GetAccount(pubkey, tx.clone()))
                .await
                .map_err(|e| BotError::Rpc(format!("发送批量账户请求失败: {}", e)))?;
        }
        drop(tx);
        let mut accounts = Vec::new();
        while let Some(result) = rx.recv().await {
            accounts.push(result?);
        }
        Ok(accounts)
    }

    /// 获取当前 slot
    pub async fn get_slot(&self) -> Result<u64, BotError> {
        let (tx, mut rx) = mpsc::channel(1);
        self.sender
            .send(RpcRequest::GetSlot(tx))
            .await
            .map_err(|e| BotError::Rpc(format!("发送 slot 请求失败: {}", e)))?;
        rx.recv()
            .await
            .ok_or(BotError::Rpc("接收 slot 响应失败".into()))?
    }

    /// 发送并确认交易
    pub async fn send_and_confirm_transaction(
        &self,
        tx: VersionedTransaction,
    ) -> Result<Signature, BotError> {
        let (tx_channel, mut rx) = mpsc::channel(1);
        self.sender
            .send(RpcRequest::SendAndConfirmTransaction(tx, tx_channel))
            .await
            .map_err(|e| BotError::Rpc(format!("发送确认交易请求失败: {}", e)))?;
        rx.recv()
            .await
            .ok_or(BotError::Rpc("接收确认交易响应失败".into()))?
    }

    /// 使用指定配置发送交易
    pub async fn send_transaction_with_config(
        &self,
        tx: VersionedTransaction,
        config: RpcSendTransactionConfig,
    ) -> Result<Signature, BotError> {
        let (tx_channel, mut rx) = mpsc::channel(1);
        self.sender
            .send(RpcRequest::SendTransactionWithConfig(tx, config, tx_channel))
            .await
            .map_err(|e| BotError::Rpc(format!("发送配置交易请求失败: {}", e)))?;
        rx.recv()
            .await
            .ok_or(BotError::Rpc("接收配置交易响应失败".into()))?
    }

    /// 确认交易状态
    pub async fn confirm_transaction(&self, signature: Signature) -> Result<bool, BotError> {
        let (tx_channel, mut rx) = mpsc::channel(1);
        self.sender
            .send(RpcRequest::ConfirmTransaction(signature, tx_channel))
            .await
            .map_err(|e| BotError::Rpc(format!("发送确认请求失败: {}", e)))?;
        rx.recv()
            .await
            .ok_or(BotError::Rpc("接收确认响应失败".into()))?
    }

    /// 获取最近的优先级费用
    pub async fn get_recent_gas_fees(&self, address: &[Pubkey]) -> Result<Vec<RpcPrioritizationFee>, BotError> {
        let address_cloned = address.to_vec();
        let recent_fees: Vec<RpcPrioritizationFee> = self
            .call(move |client| {
                client
                    .get_recent_prioritization_fees(&address_cloned)
                    .map_err(|e| BotError::Rpc(e.to_string()))
            })
            .await?;
        Ok(recent_fees)
    }
}

/// 异步任务循环，处理 RPC 请求
async fn run_request_loop(
    clients: Arc<Vec<Arc<RpcClient>>>,
    mut receiver: Receiver<RpcRequest>,
    client_index: Arc<AtomicUsize>,
    config: Arc<Config>,
    clients_status: Arc<Mutex<Vec<ClientStatus>>>,
) {
    // 启动健康检查任务
    let clients_clone = Arc::clone(&clients);
    let clients_status_clone = Arc::clone(&clients_status);
    tokio::spawn(async move {
        loop {
            check_clients_health(&clients_clone, &clients_status_clone).await;
            sleep(Duration::from_secs(30)).await;
        }
    });

    while let Some(request) = receiver.recv().await {
        let max_retries = config
            .spam
            .as_ref()
            .and_then(|spam| spam.max_retries)
            .unwrap_or(3) as usize;
        let index = select_client(&clients_status);

        match request {
            RpcRequest::GetLatestBlockhash(response_tx) => {
                let result = with_retry(
                    |client| client.get_latest_blockhash().map_err(|e| BotError::Rpc(e.to_string())),
                    max_retries,
                    &clients,
                    &clients_status,
                    index,
                )
                .await;
                update_status(&clients_status, index, &result);
                let _ = response_tx.send(result).await;
            }
            RpcRequest::SendTransaction(tx, response_tx) => {
                let result = with_retry(
                    |client| client.send_transaction(&tx).map_err(|e| BotError::Rpc(e.to_string())),
                    max_retries,
                    &clients,
                    &clients_status,
                    index,
                )
                .await;
                update_status(&clients_status, index, &result);
                let _ = response_tx.send(result).await;
            }
            RpcRequest::Call(f, response_tx) => {
                let result = with_retry(
                    |client| (f)(client),
                    max_retries,
                    &clients,
                    &clients_status,
                    index,
                )
                .await;
                update_status(&clients_status, index, &result);
                let _ = response_tx.send(result).await;
            }
            RpcRequest::GetAccount(pubkey, response_tx) => {
                let result = with_retry(
                    |client| client.get_account(&pubkey).map_err(|e| BotError::Rpc(e.to_string())),
                    max_retries,
                    &clients,
                    &clients_status,
                    index,
                )
                .await;
                update_status(&clients_status, index, &result);
                let _ = response_tx.send(result).await;
            }
            RpcRequest::GetSlot(response_tx) => {
                let result = with_retry(
                    |client| client.get_slot().map_err(|e| BotError::Rpc(e.to_string())),
                    max_retries,
                    &clients,
                    &clients_status,
                    index,
                )
                .await;
                update_status(&clients_status, index, &result);
                let _ = response_tx.send(result).await;
            }
            RpcRequest::SendAndConfirmTransaction(tx, response_tx) => {
                let result = with_retry(
                    |client| {
                        client
                            .send_and_confirm_transaction(&tx)
                            .map_err(|e| BotError::Rpc(e.to_string()))
                    },
                    max_retries,
                    &clients,
                    &clients_status,
                    index,
                )
                .await;
                update_status(&clients_status, index, &result);
                let _ = response_tx.send(result).await;
            }
            RpcRequest::SendTransactionWithConfig(tx, config_transaction, response_tx) => {
                let result = with_retry(
                    |client| {
                        client
                            .send_transaction_with_config(&tx, config_transaction.clone())
                            .map_err(|e| BotError::Rpc(e.to_string()))
                    },
                    max_retries,
                    &clients,
                    &clients_status,
                    index,
                )
                .await;
                update_status(&clients_status, index, &result);
                let _ = response_tx.send(result).await;
            }
            RpcRequest::ConfirmTransaction(signature, response_tx) => {
                let result = with_retry(
                    |client| {
                        client
                            .confirm_transaction(&signature)
                            .map_err(|e| BotError::Rpc(e.to_string()))
                    },
                    max_retries,
                    &clients,
                    &clients_status,
                    index,
                )
                .await;
                update_status(&clients_status, index, &result);
                let _ = response_tx.send(result).await;
            }
        }
    }
}

/// 健康检查：定期检测客户端可用性
async fn check_clients_health(
    clients: &Arc<Vec<Arc<RpcClient>>>,
    clients_status: &Arc<Mutex<Vec<ClientStatus>>>,
) {
    for (i, client) in clients.iter().enumerate() {
        let start = Instant::now();
        let result = client.get_slot();
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        {
            let mut status = clients_status.lock().unwrap();
            status[i].last_checked = Instant::now();
            status[i].update_response_time(elapsed_ms);

            match result {
                Ok(_) => {
                    status[i].is_healthy = true;
                    status[i].success_count += 1;
                    info!("客户端 {} 健康，响应时间: {:.2}ms", i, elapsed_ms);
                }
                Err(e) => {
                    status[i].is_healthy = false;
                    status[i].failure_count += 1;
                    warn!("客户端 {} 不可用: {}", i, e);
                }
            }
        }
    }
}

/// 加权随机选择客户端
fn select_client(clients_status: &Arc<Mutex<Vec<ClientStatus>>>) -> usize {
    let status = clients_status.lock().unwrap();
    let total_weight: f64 = status.iter().map(|s| s.weight()).sum();

    if total_weight == 0.0 {
        return rand::thread_rng().gen_range(0..status.len());
    }

    let mut rng = rand::thread_rng();
    let rand_weight = rng.gen_range(0.0..total_weight);
    let mut cumulative = 0.0;

    for (i, s) in status.iter().enumerate() {
        cumulative += s.weight();
        if rand_weight <= cumulative {
            return i;
        }
    }

    status.len() - 1
}

/// 更新客户端状态
fn update_status<T>(
    clients_status: &Arc<Mutex<Vec<ClientStatus>>>,
    index: usize,
    result: &Result<T, BotError>,
) {
    let mut status = clients_status.lock().unwrap();
    match result {
        Ok(_) => {
            status[index].success_count += 1;
        }
        Err(e) => {
            status[index].failure_count += 1;
            if e.to_string().contains("AccountNotFound") {
                status[index].is_healthy = false;
            }
        }
    }
}

/// 重试逻辑：支持指数退避和客户端切换
async fn with_retry<F, T>(
    mut action: F,
    max_retries: usize,
    clients: &Arc<Vec<Arc<RpcClient>>>,
    clients_status: &Arc<Mutex<Vec<ClientStatus>>>,
    initial_index: usize,
) -> Result<T, BotError>
where
    F: FnMut(&RpcClient) -> Result<T, BotError>,
    T: Send + 'static,
{
    let max_retries = max_retries.max(3);
    let mut attempts = 0;
    let mut current_index = initial_index;

    while attempts <= max_retries {
        let start = Instant::now();
        let client = &clients[current_index];

        match action(client) {
            Ok(result) => {
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                {
                    let mut status = clients_status.lock().unwrap();
                    status[current_index].update_response_time(elapsed_ms);
                    status[current_index].success_count += 1;
                }
                return Ok(result);
            }
            Err(e) => {
                if e.to_string().contains("AccountNotFound") {
                    return Err(BotError::Rpc("账户不存在".into()));
                }
                attempts += 1;
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                {
                    let mut status = clients_status.lock().unwrap();
                    status[current_index].update_response_time(elapsed_ms);
                    status[current_index].failure_count += 1;
                }

                if attempts > max_retries {
                    return Err(BotError::Rpc(format!("重试 {} 次后失败: {}", max_retries, e)));
                }

                let retry_delay_ms = 50.0 * (1 << attempts) as f64;
                warn!(
                    "客户端 {} 尝试 {} 失败: {}. 延迟 {}ms 后重试...",
                    current_index, attempts, e, retry_delay_ms
                );
                sleep(Duration::from_millis(retry_delay_ms as u64)).await;

                current_index = select_client(clients_status);
            }
        }
    }

    Err(BotError::Rpc("重试逻辑未正确终止".into()))
}
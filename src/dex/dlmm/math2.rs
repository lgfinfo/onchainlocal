use anchor_lang::AccountDeserialize;
use anchor_spl::token::Mint;
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use solana_program::pubkey::Pubkey;
use std::collections::HashMap;

pub struct PriceCalculator {
    mint_cache: HashMap<Pubkey, Mint>,
    base_cache: HashMap<u16, u128>, // 缓存 bin_step 对应的 base 值
}

impl PriceCalculator {
    pub fn new() -> Self {
        PriceCalculator {
            mint_cache: HashMap::new(),
            base_cache: HashMap::new(),
        }
    }

    pub async fn calculate_price(
        &mut self,
        rpc_client: &rpc::RpcManager,
        pool_pubkey: Pubkey,
    ) -> Result<Decimal> {
        // 获取 LbPair 账户
        let pool_account = rpc_client.get_account(pool_pubkey).await?;
        let amminfo = DlmmInfo::load_checked(&pool_account.data)?;
        let lb_pair = amminfo.lb_pair;

        // 获取代币 Mint 数据（从缓存或 RPC）
        let token_x_mint = lb_pair.token_x_mint;
        let token_y_mint = lb_pair.token_y_mint;
        let x_mint = self.get_or_fetch_mint(rpc_client, token_x_mint).await?;
        let y_mint = self.get_or_fetch_mint(rpc_client, token_y_mint).await?;

        // 计算 Q64x64 价格
        let bin_step = lb_pair.bin_step;
        let active_id = lb_pair.active_id;
        let base = self.get_or_compute_base(bin_step)?;
        let q64x64_price = pow(base, active_id).context("Price calculation overflow")?;

        // 转换为十进制价格
        let decimal_price_per_lamport = q64x64_price_to_decimal(q64x64_price)
            .context("Failed to convert Q64x64 to decimal")?;

        // 调整为每代币价格
        let token_price = price_per_lamport_to_price_per_token(
            decimal_price_per_lamport.to_f64().context("Decimal to f64 failed")?,
            x_mint.decimals,
            y_mint.decimals,
        )
        .context("Failed to convert to token price")?;

        Ok(token_price)
    }

    async fn get_or_fetch_mint(
        &mut self,
        rpc_client: &rpc::RpcManager,
        mint: Pubkey,
    ) -> Result<Mint> {
        if let Some(mint_data) = self.mint_cache.get(&mint) {
            return Ok(mint_data.clone());
        }
        let account = rpc_client.get_account(mint).await?;
        let mint_data = Mint::try_deserialize(&mut account.data.as_ref())?;
        self.mint_cache.insert(mint, mint_data.clone());
        Ok(mint_data)
    }

    fn get_or_compute_base(&mut self, bin_step: u16) -> Result<u128> {
        if let Some(base) = self.base_cache.get(&bin_step) {
            return Ok(*base);
        }
        let base = precompute_base(bin_step)?;
        self.base_cache.insert(bin_step, base);
        Ok(base)
    }
}

fn precompute_base(bin_step: u16) -> Result<u128> {
    let bps = u128::from(bin_step)
        .checked_shl(SCALE_OFFSET.into())
        .unwrap()
        .checked_div(BASIS_POINT_MAX as u128)
        .context("overflow")?;
    ONE.checked_add(bps).context("overflow")
}

// 测试用例
#[tokio::test(flavor = "multi_thread")]
async fn test_optimized_price() -> Result<()> {
    let config = config::get_config();
    let dlmm_pool_pubkey = "Fsb7kScDsQ28VoTyqZZLbLuUAzNZz8DaJgcGXJQTSuot".parse().unwrap();
    let rpc_client = rpc::RpcManager::new(config);

    let mut calculator = PriceCalculator::new();
    let token_price = calculator.calculate_price(&rpc_client, dlmm_pool_pubkey).await?;

    println!("Current price: {}", token_price);
    Ok(())
}
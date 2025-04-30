
pub const SCALE_OFFSET: u8 = 64;

pub const BASIS_POINT_MAX: i32 = 10000;

pub const SLOT_BUFFER: u64 = 9000;

pub const TIME_BUFFER: u64 = 3600;

pub const ONE: u128 = 1u128 << SCALE_OFFSET;

const MAX_EXPONENTIAL: u32 = 0x80000; // 1048576
pub const FEE_PRECISION: u64 = 1_000_000_000;

use anyhow::{anyhow, Result};

use dlmm_interface::Rounding;
use rust_decimal::MathematicalOps;
use rust_decimal::{
    prelude::{FromPrimitive, ToPrimitive},
    Decimal,
};

pub fn compute_base_factor_from_fee_bps(bin_step: u16, fee_bps: u16) -> Result<(u16, u8)> {
    let computed_base_factor = fee_bps as f64 * 10_000.0f64 / bin_step as f64;

    if computed_base_factor > u16::MAX as f64 {
        let mut truncated_base_factor = computed_base_factor;
        let mut base_power_factor = 0u8;
        loop {
            if truncated_base_factor < u16::MAX as f64 {
                break;
            }

            let remainder = truncated_base_factor % 10.0;
            if remainder == 0.0 {
                base_power_factor += 1;
                truncated_base_factor /= 10.0;
            } else {
                return Err(anyhow!("have decimals"));
            }
        }

        Ok((truncated_base_factor as u16, base_power_factor))
    } else {
        // Sanity check
        let casted_base_factor = computed_base_factor as u16 as f64;
        if casted_base_factor != computed_base_factor {
            if casted_base_factor == u16::MAX as f64 {
                return Err(anyhow!("overflow"));
            }

            if casted_base_factor == 0.0f64 {
                return Err(anyhow!("underflow"));
            }

            if computed_base_factor.fract() != 0.0 {
                return Err(anyhow!("have decimals"));
            }

            return Err(anyhow!("unknown error"));
        }

        Ok((computed_base_factor as u16, 0u8))
    }
}

pub fn get_precise_id_from_price(bin_step: u16, price: &Decimal) -> Option<i32> {
    let bps = Decimal::from_u16(bin_step)?.checked_div(Decimal::from_i32(BASIS_POINT_MAX)?)?;
    let base = Decimal::ONE.checked_add(bps)?;

    let id = price.log10().checked_div(base.log10())?.to_f64()?;
    let trimmed_id = id as i32;
    let trimmed_id_f64 = trimmed_id as f64;

    if trimmed_id_f64 == id {
        id.to_i32()
    } else {
        None
    }
}

/// Calculate the bin id based on price. If the bin id is in between 2 bins, it will round up.
pub fn get_id_from_price(bin_step: u16, price: &Decimal, rounding: Rounding) -> Option<i32> {
    let bps = Decimal::from_u16(bin_step)?.checked_div(Decimal::from_i32(BASIS_POINT_MAX)?)?;
    let base = Decimal::ONE.checked_add(bps)?;

    let id = match rounding {
        Rounding::Down => price.log10().checked_div(base.log10())?.floor(),
        Rounding::Up => price.log10().checked_div(base.log10())?.ceil(),
    };

    id.to_i32()
}

/// Convert Q64xQ64 price to human readable decimal. This is price per lamport.
pub fn q64x64_price_to_decimal(q64x64_price: u128) -> Option<Decimal> {
    let q_price = Decimal::from_u128(q64x64_price)?;
    let scale_off = Decimal::TWO.powu(SCALE_OFFSET.into());
    q_price.checked_div(scale_off)
}

/// price_per_lamport = price_per_token * 10 ** quote_token_decimal / 10 ** base_token_decimal
pub fn price_per_token_to_per_lamport(
    price_per_token: f64,
    base_token_decimal: u8,
    quote_token_decimal: u8,
) -> Option<Decimal> {
    let price_per_token = Decimal::from_f64(price_per_token)?;
    price_per_token
        .checked_mul(Decimal::TEN.powu(quote_token_decimal.into()))?
        .checked_div(Decimal::TEN.powu(base_token_decimal.into()))
}

/// price_per_token = price_per_lamport * 10 ** base_token_decimal / 10 ** quote_token_decimal, Solve for price_per_lamport
pub fn price_per_lamport_to_price_per_token(
    price_per_lamport: f64,
    base_token_decimal: u8,
    quote_token_decimal: u8,
) -> Option<Decimal> {
    let one_ui_base_token_amount = Decimal::TEN.powu(base_token_decimal.into());
    let one_ui_quote_token_amount = Decimal::TEN.powu(quote_token_decimal.into());
    let price_per_lamport = Decimal::from_f64(price_per_lamport)?;

    one_ui_base_token_amount
        .checked_mul(price_per_lamport)?
        .checked_div(one_ui_quote_token_amount)
}
pub fn pow2(base: u128, exp: i32) -> Option<u128> {
    let mut invert = exp.is_negative();
    if exp == 0 {
        return Some(1u128 << 64);
    }
    let exp = if invert { exp.abs() as u32 } else { exp as u32 };
    if exp >= MAX_EXPONENTIAL {
        return None;
    }

    let mut squared_base = base;
    let mut result = ONE;

    if squared_base >= result {
        squared_base = u128::MAX.checked_div(squared_base)?;
        invert = !invert;
    }

    let mut exp = exp;
    while exp > 0 {
        if exp & 1 > 0 {
            result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET;
        }
        squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
        exp >>= 1;
    }

    if result == 0 {
        return None;
    }
    if invert {
        result = u128::MAX.checked_div(result)?;
    }
    Some(result)
}
pub fn fee_rate_to_fee_pct(fee_rate: u128) -> Option<Decimal> {
    let fee_rate = Decimal::from_u128(fee_rate)?.checked_div(Decimal::from(FEE_PRECISION))?;
    fee_rate.checked_mul(Decimal::ONE_HUNDRED)
}

pub fn pow(base: u128, exp: i32) -> Option<u128> {
    // If exponent is negative. We will invert the result later by 1 / base^exp.abs()
    let mut invert = exp.is_negative();

    // When exponential is 0, result will always be 1
    if exp == 0 {
        return Some(1u128 << 64);
    }

    // Make the exponential positive. Which will compute the result later by 1 / base^exp
    let exp: u32 = if invert { exp.abs() as u32 } else { exp as u32 };

    // No point to continue the calculation as it will overflow the maximum value Q64.64 can support
    if exp >= MAX_EXPONENTIAL {
        return None;
    }

    let mut squared_base = base;
    let mut result = ONE;

    // When multiply the base twice, the number of bits double from 128 -> 256, which overflow.
    // The trick here is to inverse the calculation, which make the upper 64 bits (number bits) to be 0s.
    // For example:
    // let base = 1.001, exp = 5
    // let neg = 1 / (1.001 ^ 5)
    // Inverse the neg: 1 / neg
    // By using a calculator, you will find out that 1.001^5 == 1 / (1 / 1.001^5)
    if squared_base >= result {
        // This inverse the base: 1 / base
        squared_base = u128::MAX.checked_div(squared_base)?;
        // If exponent is negative, the above already inverted the result. Therefore, at the end of the function, we do not need to invert again.
        invert = !invert;
    }

    // The following code is equivalent to looping through each binary value of the exponential.
    // As explained in MAX_EXPONENTIAL, 19 exponential bits are enough to covert the full bin price.
    // Therefore, there will be 19 if statements, which similar to the following pseudo code.
    /*
        let mut result = 1;
        while exponential > 0 {
            if exponential & 1 > 0 {
                result *= base;
            }
            base *= base;
            exponential >>= 1;
        }
    */

    // From right to left
    // squared_base = 1 * base^1
    // 1st bit is 1
    if exp & 0x1 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    // squared_base = base^2
    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    // 2nd bit is 1
    if exp & 0x2 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    // Example:
    // If the base is 1.001, exponential is 3. Binary form of 3 is ..0011. The last 2 1's bit fulfill the above 2 bitwise condition.
    // The result will be 1 * base^1 * base^2 == base^3. The process continues until reach the 20th bit

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x4 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x8 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x10 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x20 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x40 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x80 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x100 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x200 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x400 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x800 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x1000 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x2000 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x4000 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x8000 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x10000 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x20000 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    squared_base = (squared_base.checked_mul(squared_base)?) >> SCALE_OFFSET;
    if exp & 0x40000 > 0 {
        result = (result.checked_mul(squared_base)?) >> SCALE_OFFSET
    }

    // Stop here as the next is 20th bit, which > MAX_EXPONENTIAL
    if result == 0 {
        return None;
    }

    if invert {
        result = u128::MAX.checked_div(result)?;
    }

    Some(result)
}
use anyhow::Context;
pub fn get_price_from_id(active_id: i32, bin_step: u16) -> Result<u128> {
    let bps = u128::from(bin_step)
        .checked_shl(SCALE_OFFSET.into())
        .unwrap()
        .checked_div(BASIS_POINT_MAX as u128)
        .context("overflow")?;

    let base = ONE.checked_add(bps).context("overflow")?;

    pow(base, active_id).context("overflow")
}

use anchor_lang::AnchorSerialize;
use anchor_spl::token::Mint;
use crate::dex::dlmm::dlmm_info::*;
use rust_decimal::prelude::*;
use crate::config;
use crate::rpc;
use anchor_lang::AccountDeserialize;



#[tokio::test(flavor = "multi_thread")]
async fn test_math()-> Result<(), anyhow::Error> {
    let config = config::get_config();
    let dlmm_pool_pubkey ="Fsb7kScDsQ28VoTyqZZLbLuUAzNZz8DaJgcGXJQTSuot".parse().unwrap();
    let rpc_client = rpc::RpcManager::new(config);
    let account = rpc_client.get_account(dlmm_pool_pubkey).await.unwrap();

    let amminfo = match DlmmInfo::load_checked(&account.data) {
        Ok(amm_info) => amm_info,
        Err(e) => {
            return Err(e);
        }
    };
    let lb_pair_state = amminfo.lb_pair;
    println!("amm_info {:?}", lb_pair_state);
    let mut accounts=vec![];
    accounts.push(lb_pair_state.token_x_mint);
    accounts.push(lb_pair_state.token_y_mint);
    let accounts_data = rpc_client
        .get_accounts(accounts)
        .await
        .unwrap();

    let [token_x_account,token_y_account] = accounts_data.as_slice() else {
        return Err(anyhow::anyhow!("get token account data error"));
    };

    let x_mint = Mint::try_deserialize(&mut token_x_account.data.as_ref()).unwrap();
    let y_mint = Mint::try_deserialize(&mut token_y_account.data.as_ref()).unwrap();

    let q64x64_price = get_price_from_id(lb_pair_state.active_id, lb_pair_state.bin_step).unwrap();
    let decimal_price_per_lamport = q64x64_price_to_decimal(q64x64_price).unwrap();

    let token_price = price_per_lamport_to_price_per_token(
        decimal_price_per_lamport
            .to_f64().unwrap(),
        x_mint.decimals,
        y_mint.decimals,
    ).unwrap();

    // let base_fee_rate = fee_rate_to_fee_pct(lb_pair_state.get_total_fee().unwrap())
    //     .context("get_total_fee convert to percentage overflow")
    //     .unwrap();
    // let variable_fee_rate = fee_rate_to_fee_pct(lb_pair_state.get_variable_fee().unwrap())
    //     .context("get_total_fee convert to percentage overflow")
    //     .unwrap();
    // let current_fee_rate = fee_rate_to_fee_pct(lb_pair_state.get_total_fee().unwrap())
    //     .context("get_total_fee convert to percentage overflow")
    //     .unwrap();

    println!("Current price {}", token_price);
    // println!("Base fee rate {}%", base_fee_rate);
    // println!("Volatile fee rate {}%", variable_fee_rate);
    // println!("Current fee rate {}%", current_fee_rate);
    // assert_eq!(1,1);
    Ok(())
}

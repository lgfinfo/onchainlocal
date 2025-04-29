use log::info;
use onchainlocal::{
    executor::run_bot,
    setup::{initialize_context, load_mint_list},
    generate::pull_token_pools
};
use onchainlocal::error::BotError;
/// 初始化日志
fn init_logging() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
}
/// 主函数，执行套利机器人逻辑
#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), BotError> {
    init_logging();
    info!("启动链上套利机器人...");
    
    let context = initialize_context().await?;
    let mint_list = load_mint_list(&context).await?;
    let token_pools = pull_token_pools(&context,&mint_list).await?;
    run_bot(&context,&token_pools).await?;

    Ok(())
}
// 文件: src/error.rs
// ```rust
#[derive(thiserror::Error, Debug)]
pub enum BotError {
    #[error("Invalid token program: {0}")]
    InvalidTokenProgram(String),
    #[error("Invalid mint account")]
    InvalidMint,
    #[error("API error: {0}")]
    Api(String),
    #[error("RPC error: {0}")]
    Rpc(String),
    #[error("Pool query error for {protocol}: {message}")]
    PoolQuery {
        protocol: String,
        message: String,
        details: Option<serde_json::Value>,
    },
    #[error("Transaction error: {0}")]
    Transaction(String),
    #[error("Instruction error: {0}")]
    Instruction(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serde error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Database error: {0}")]
    Database(String),
    #[error("semaphore error: {0}")]
    Concurrency(String),
}
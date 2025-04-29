// 文件: src/api_config.rs
// ```rust
use serde::{Deserialize, Serialize};
use reqwest::{Client, header::{HeaderMap, HeaderName, HeaderValue}};
use std::error::Error;
use std::fs::File;
use std::io::Write;
use crate::config::MintConfig;
use crate::shared_context::Context;
// 扩展数据结构
#[derive(Debug, Serialize, Deserialize)]
struct ExtendedPoolData {
    timestamp: u64
}
// 将数据保存到 JSON 文件
fn save_to_json_file<T: Serialize>(data: &T, filename: &str) -> Result<(), Box<dyn Error>> {
    // 序列化为格式化的 JSON 字符串
    let json_str = serde_json::to_string_pretty(data)?;
    // 创建或打开文件并写入
    let mut file = File::create(filename)?;
    file.write_all(json_str.as_bytes())?;

    Ok(())
}

pub async fn get_api_config(context:&Context) -> Result<Vec<MintConfig>, Box<dyn Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/json"),
    );
    // 假设 context.wali_config.api.url 是 Arc<String> 类型，使用 clone 方法避免移动
    let url = context.wali_config.api.url.clone();
    let client = Client::new();

    // 如果提供了 POST 参数，则发送 POST 请求，否则发送 GET 请求
    let response = if context.wali_config.api.diy_mints_open {
        let json_post = serde_json::json!({
            "diy_mints": context.wali_config.api.diy_mints,
            "forbidden_mints": context.wali_config.api.forbidden_mints,
        });
        client
            .post(url)
            .headers(headers)
            .json(&json_post)
            .send()
            .await?
    } else {
        client
            .post(url)
            .headers(headers)
            .send()
            .await?
    };

    // 检查状态码
    if !response.status().is_success() {
        return Err(format!(
            "Request failed with status: {}",
            response.status()
        ).into());
    }

    // 获取响应内容
    let text = response.text().await?;

    // 解析 JSON
    let pool_entries: Vec<MintConfig> = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse JSON: {}", e))?;

    save_to_json_file(&pool_entries, "pool_data_output.json")?;

    Ok(pool_entries)
}
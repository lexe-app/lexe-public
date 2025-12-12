uniffi::setup_scaffolding!("lexe");

#[uniffi::export]
fn add(a: u32, b: u32) -> u32 {
    a + b
}

pub mod hex {
    #[uniffi::export]
    pub fn hex_encode(data: &[u8]) -> String {
        hex::encode(data)
    }
}

// #[uniffi::export(async_runtime = "tokio")]
// pub async fn latest_enclave() -> Result<String, AnyhowError> {
//     let deploy_env = DeployEnv::Staging;
//     let gateway_url = "https://lexe-staging-sgx.uswest2.staging.lexe.app";
//     let user_agent = "sdk-python/0.1.0";
//     let client = app_rs::client::GatewayClient::new(
//         deploy_env,
//         gateway_url.to_string(),
//         user_agent,
//     )?;
//
//     let enclaves = client.current_enclaves().await.context("oops")?.enclaves;
//
//     Ok(format!("{enclaves:?}"))
// }

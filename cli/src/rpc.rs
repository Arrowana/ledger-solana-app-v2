use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::constants::SQUADS_PROGRAM_ID;

#[derive(Clone)]
pub struct RpcClient {
    url: String,
    http: Client,
}

impl RpcClient {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            http: Client::new(),
        }
    }

    pub fn latest_blockhash(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Response {
            result: Option<ResultEnvelope>,
            error: Option<Value>,
        }

        #[derive(Deserialize)]
        struct ResultEnvelope {
            value: BlockhashValue,
        }

        #[derive(Deserialize)]
        struct BlockhashValue {
            blockhash: String,
        }

        let response: Response = self.rpc("getLatestBlockhash", json!([{ "commitment": "confirmed" }]))?;
        if let Some(error) = response.error {
            bail!("RPC getLatestBlockhash failed: {error}");
        }
        Ok(response
            .result
            .context("missing latest blockhash result")?
            .value
            .blockhash)
    }

    pub fn validate_multisig(&self, multisig: &str) -> Result<()> {
        #[derive(Deserialize)]
        struct Response {
            result: Option<AccountInfoEnvelope>,
            error: Option<Value>,
        }

        #[derive(Deserialize)]
        struct AccountInfoEnvelope {
            value: Option<AccountInfo>,
        }

        #[derive(Deserialize)]
        struct AccountInfo {
            owner: String,
        }

        let response: Response = self.rpc(
            "getAccountInfo",
            json!([multisig, { "commitment": "confirmed", "encoding": "base64" }]),
        )?;
        if let Some(error) = response.error {
            bail!("RPC getAccountInfo failed: {error}");
        }
        let account = response
            .result
            .context("missing account info result")?
            .value
            .context("multisig account not found")?;
        if account.owner != SQUADS_PROGRAM_ID {
            bail!(
                "multisig account is not owned by the Squads program: {}",
                account.owner
            );
        }
        Ok(())
    }

    fn rpc<T: for<'de> Deserialize<'de>>(&self, method: &str, params: Value) -> Result<T> {
        let response = self
            .http
            .post(&self.url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": method,
                "params": params,
            }))
            .send()
            .with_context(|| format!("RPC request failed for {method}"))?
            .error_for_status()
            .with_context(|| format!("RPC HTTP error for {method}"))?;

        response
            .json::<T>()
            .with_context(|| format!("invalid RPC response for {method}"))
    }
}


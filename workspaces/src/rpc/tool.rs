// TODO: Remove this when near-jsonrpc-client crate no longer defaults to deprecation for
//       warnings about unstable API.
#![allow(deprecated)]

use std::collections::HashMap;
use std::convert::TryInto;
use std::path::PathBuf;

use chrono::Utc;
use rand::Rng;
use url::Url;

use near_crypto::PublicKey;
use near_jsonrpc_client::{
    errors::{JsonRpcError, JsonRpcServerError},
    methods, JsonRpcClient,
};
use near_jsonrpc_primitives::types::{query::QueryResponseKind, transactions::RpcTransactionError};
use near_primitives::hash::CryptoHash;
use near_primitives::transaction::SignedTransaction;
use near_primitives::types::{AccountId, BlockHeight, Finality};
use near_primitives::views::{AccessKeyView, FinalExecutionOutcomeView, QueryRequest, StateItem};

use crate::runtime::context::MISSING_RUNTIME_ERROR;

fn rt_current_addr() -> String {
    crate::runtime::context::current()
        .expect(MISSING_RUNTIME_ERROR)
        .rpc_addr()
}

pub(crate) fn json_client() -> JsonRpcClient {
    JsonRpcClient::connect(&rt_current_addr())
}

pub(crate) async fn access_key(
    account_id: AccountId,
    pk: PublicKey,
) -> Result<(AccessKeyView, BlockHeight, CryptoHash), String> {
    let query_resp = json_client()
        .call(&methods::query::RpcQueryRequest {
            block_reference: Finality::Final.into(),
            request: QueryRequest::ViewAccessKey {
                account_id,
                public_key: pk,
            },
        })
        .await
        .map_err(|err| format!("Failed to fetch public key info: {:?}", err))?;

    match query_resp.kind {
        QueryResponseKind::AccessKey(access_key) => {
            Ok((access_key, query_resp.block_height, query_resp.block_hash))
        }
        _ => Err("Could not retrieve access key".to_owned()),
    }
}

pub(crate) async fn send_tx(tx: SignedTransaction) -> Result<FinalExecutionOutcomeView, String> {
    let client = json_client();
    let transaction_info_result = loop {
        let transaction_info_result = client
            .clone()
            .call(&methods::broadcast_tx_commit::RpcBroadcastTxCommitRequest {
                signed_transaction: tx.clone(),
            })
            .await;

        if let Err(ref err) = transaction_info_result {
            if matches!(
                err,
                JsonRpcError::ServerError(JsonRpcServerError::HandlerError(
                    RpcTransactionError::TimeoutError
                ))
            ) {
                eprintln!("transaction timeout: {:?}", err);
                continue;
            }
        }

        break transaction_info_result;
    };

    // TODO: remove this after adding exponential backoff
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    transaction_info_result.map_err(|e| format!("Error transaction: {:?}", e))
}

pub(crate) fn credentials_filepath(account_id: AccountId) -> anyhow::Result<PathBuf> {
    let mut path = crate::runtime::context::current()
        .expect(MISSING_RUNTIME_ERROR)
        .keystore_path()?;

    // Create this path's directories if they don't exist:
    std::fs::create_dir_all(path.clone())?;

    path.push(format!("{}.json", account_id));
    Ok(path)
}

/// Convert `StateItem`s over to a Map<data_key, value_bytes> representation.
/// Assumes key and value are base64 encoded, so this also decodes them.
pub(crate) fn into_state_map(
    state_items: &[StateItem],
) -> anyhow::Result<HashMap<String, Vec<u8>>> {
    let decode = |s: &StateItem| {
        Ok((
            String::from_utf8(base64::decode(&s.key)?)?,
            base64::decode(&s.value)?,
        ))
    };

    state_items.iter().map(decode).collect()
}

pub(crate) fn random_account_id() -> AccountId {
    let mut rng = rand::thread_rng();
    let random_num = rng.gen_range(10000000000000usize..99999999999999);
    let account_id = format!("dev-{}-{}", Utc::now().format("%Y%m%d%H%M%S"), random_num);
    let account_id: AccountId = account_id
        .try_into()
        .expect("could not convert dev account into AccountId");

    account_id
}

pub(crate) async fn url_create_account(
    helper_url: Url,
    account_id: AccountId,
    pk: PublicKey,
) -> anyhow::Result<()> {
    let helper_addr = helper_url.join("account")?;

    // TODO(maybe): need this in near-jsonrpc-client as well:
    let _resp = reqwest::Client::new()
        .post(helper_addr)
        .header("Content-Type", "application/json")
        .body(serde_json::to_vec(&serde_json::json!({
            "newAccountId": account_id.to_string(),
            "newAccountPublicKey": pk.to_string(),
        }))?)
        .send()
        .await?;

    Ok(())
}

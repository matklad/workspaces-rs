pub(crate) mod context;
pub(crate) mod local;
pub(crate) mod online;

pub use local::SandboxRuntime;
pub use online::TestnetRuntime;

use anyhow::anyhow;
use url::Url;

use std::path::{Path, PathBuf};

use near_crypto::{PublicKey, Signer};
use near_primitives::types::AccountId;
use near_primitives::views::FinalExecutionOutcomeView;

use crate::CallExecutionResult;

const SANDBOX_CREDENTIALS_DIR: &str = ".near-credentials/sandbox/";
const TESTNET_CREDENTIALS_DIR: &str = ".near-credentials/testnet/";

// TODO: implement mainnet/testnet runtimes
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) enum RuntimeFlavor {
    Mainnet,
    Testnet,
    Sandbox(u16),
}

impl RuntimeFlavor {
    pub fn rpc_addr(&self) -> String {
        match self {
            Self::Sandbox(port) => format!("http://localhost:{}", port),
            Self::Testnet => online::TestnetRuntime::RPC_URL.to_string(),
            _ => unimplemented!(),
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Sandbox(_) => "sandbox",
            Self::Mainnet => "mainnet",
            Self::Testnet => "testnet",
        }
    }

    pub fn keystore_path(&self) -> anyhow::Result<PathBuf> {
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow!("Could not get HOME_DIR".to_string()))?;
        let mut path = PathBuf::from(&home_dir);
        path.push(match self {
            Self::Sandbox(_) => SANDBOX_CREDENTIALS_DIR,
            Self::Testnet => TESTNET_CREDENTIALS_DIR,
            _ => unimplemented!(),
        });

        Ok(path)
    }

    pub fn helper_url(&self) -> Url {
        match self {
            Self::Testnet => Url::parse(online::TestnetRuntime::HELPER_URL).unwrap(),
            _ => unimplemented!(),
        }
    }

    pub async fn create_top_level_account(
        &self,
        new_account_id: AccountId,
        new_account_pk: PublicKey,
    ) -> anyhow::Result<Option<CallExecutionResult>> {
        match self {
            Self::Sandbox(_) => Ok(Some(
                local::create_top_level_account(new_account_id, new_account_pk).await?,
            )),
            Self::Testnet => {
                online::create_top_level_account(new_account_id, new_account_pk).await?;
                Ok(None)
            }
            _ => unimplemented!(),
        }
    }

    pub async fn create_tla_and_deploy(
        &self,
        new_account_id: AccountId,
        new_account_pk: PublicKey,
        signer: &dyn Signer,
        code_filepath: impl AsRef<Path>,
    ) -> anyhow::Result<FinalExecutionOutcomeView> {
        match self {
            Self::Sandbox(_) => {
                local::create_tla_and_deploy(new_account_id, new_account_pk, signer, code_filepath)
                    .await
            }
            Self::Testnet => {
                online::create_tla_and_deploy(new_account_id, new_account_pk, signer, code_filepath)
                    .await
            }
            _ => unimplemented!(),
        }
    }
}

pub(crate) fn assert_within(runtimes: &[&str]) -> bool {
    runtimes.contains(
        &crate::runtime::context::current()
            .expect(crate::runtime::context::MISSING_RUNTIME_ERROR)
            .name(),
    )
}

/// Spawn this task within a new runtime context. Useful for when trying to
/// run multiple runtimes (testnet, sandbox, ...) within the same thread.
// NOTE: this could also be equivalent to tokio::spawn as well
pub(crate) async fn scope<T>(runtime: &str, scoped_task: T) -> anyhow::Result<T::Output>
where
    T: core::future::Future + Send + 'static,
    T::Output: Send + 'static,
{
    let rt = runtime.to_string();
    let task = move || {
        // Create the relevant runtime. This is similar to how workspaces_macros
        // sets up the runtime, except we're not setting up a second runtime here.
        // Expects tokio to be used for the runtime. Might consider using
        // async_compat if we want to expose choosing the runtime to the user.
        match &*rt {
            "sandbox" => {
                let mut rt = SandboxRuntime::default();
                let _ = rt.run().unwrap();

                tokio::runtime::Handle::current().block_on(scoped_task)
            }
            "testnet" => {
                let mut rt = TestnetRuntime::default();
                let _ = rt.run().unwrap();

                tokio::runtime::Handle::current().block_on(scoped_task)
            }
            _ => unimplemented!(),
        }
    };

    tokio::task::spawn_blocking(task).await.map_err(Into::into)
}

pub async fn with_sandbox<T>(scoped_task: T) -> anyhow::Result<T::Output>
where
    T: core::future::Future + Send + 'static,
    T::Output: Send + 'static,
{
    scope("sandbox", scoped_task).await
}

pub async fn with_testnet<T>(scoped_task: T) -> anyhow::Result<T::Output>
where
    T: core::future::Future + Send + 'static,
    T::Output: Send + 'static,
{
    scope("testnet", scoped_task).await
}

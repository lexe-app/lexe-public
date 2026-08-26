//! The `unilateral-exit` subcommand.
//!
//! ## Manual end-to-end test on a local regtest cluster
//!
//! 1. Start the cluster with `just dev-run`. On a first run, provision the LSP
//!    from another terminal: `just dev-lexectl lsp provision`. If migrations
//!    fail on a stale dev DB, wipe it first with `just migrate fresh`.
//! 2. Export `DEV_GATEWAY_URL=https://localhost:4040`,
//!    `LEXE_DATA_DIR=tmp/lexe-test-data`, and a fresh `LEXE_ROOT_SEED=<64 hex
//!    chars>`, and `unset LEXE_CLIENT_CREDENTIALS_PATH` if set. Then create a
//!    user with funds in a channel:
//!
//!    ```sh
//!    $ just dev-lexe signup
//!    $ just dev-lexe node-info   # note the user_pk and node_pk
//!    $ just dev-lexectl lsp open-channel --value-sats 1000000 \
//!        --user-node-pk <node_pk>
//!    $ just dev-lexe create-invoice --amount-sats 300000
//!    $ just dev-lexectl lsp pay-invoice <invoice>
//!    $ just dev-lexectl user shutdown <user_pk>   # stop the usernode
//!    ```
//!
//! 3. Export the user's channel state from the dev DB, which holds the same
//!    ciphertext as the GDrive backups:
//!
//!    ```sh
//!    $ mkdir -p tmp/recovery
//!    $ psql "$DATABASE_URL" -Atc "SELECT encode(data, 'hex') FROM node_file
//!      WHERE user_pk = decode('<user_pk>', 'hex')
//!      AND dirname = '.' AND filename = 'channel_manager'" \
//!      | xxd -r -p > tmp/recovery/channel_manager
//!    ```
//!
//!    and likewise one file per `dirname = 'channel_monitors'` row.
//! 4. Run the exit. The esplora port is random per run, so grep the dev-run log
//!    for "esplora URL" and connect via 127.0.0.1. To exercise the coop close
//!    with the dev LSP instead, omit `--force-close-only` and skip steps 5-6.
//!
//!    ```sh
//!    $ just dev-lexe unilateral-exit --force-close-only \
//!        --channel-manager tmp/recovery/channel_manager \
//!        --monitor <path> \
//!        --esplora-urls http://127.0.0.1:<esplora port>
//!    ```
//!
//! 5. A force close needs confirmed on-chain funds for anchor fees, so the run
//!    pauses and prints `ACTION REQUIRED`. Deposit to the printed address:
//!    `just dev-bitcoin-cli sendtoaddress <address> 0.001`, then `just
//!    dev-bitcoin-cli -generate 3`.
//! 6. The force-closed balance then waits out the 432-block contest delay, so
//!    fast-forward the chain: `just dev-bitcoin-cli -generate 500`.

use std::{
    env,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use anyhow::{Context, anyhow};
use bitcoin::address::NetworkUnchecked;
use clap::Parser;
use lexe::{
    config::{Network, WalletEnvConfig, WalletEnvDbConfig, WalletUserDbConfig},
    types::{auth::Credentials, bitcoin::Amount},
};
use lexe_common::{constants, ln::addr::LxSocketAddress, or_env::OrEnvExt};
use lexe_payment_uri_core::Bip321Uri;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use tokio::sync::watch;
use unilateral_exit::{
    RecoveryNode,
    types::{
        AnchorFundsNeeded, CloseMode, LexeLsp, RecoveryArgs, RecoveryProgress,
    },
};

use crate::{HELP_TEMPLATE, helpers};

#[derive(Parser)]
#[command(
    about = "Recover all funds without Lexe: close channels and sweep",
    long_about = "Recovers all funds from your node without requiring the\n\
        assistance or cooperation of the Lexe company.\n\
        \n\
        Runs a local Lightning node from your Google Drive channel state\n\
        backup. First attempts to cooperatively close all channels via\n\
        Lexe's LSP; if the LSP is unreachable, force closes instead. All\n\
        funds are swept to the on-chain wallet derived from your root seed\n\
        and the command waits until they are confirmed.\n\
        \n\
        Download the backup files from the `LexeData` folder in your Google\n\
        Drive (drive.google.com):\n\
        - `channel_manager`: pass via --channel-manager\n\
        - `channel_monitors <name>`: pass each file via --monitor\n\
        - `password_encrypted_root_seed`: if you don't have your\n\
          seedphrase, pass via --password-encrypted-root-seed along with\n\
          your backup password.\n\
        \n\
        Force closes must wait out the channel contest delay (up to ~4\n\
        days) before funds can be swept, so this command may run for a\n\
        long time. Progress is saved in the recovery dir: if interrupted,\n\
        just re-run with the same arguments.\n\
        \n\
        Force closes also require a small on-chain balance to pay fees.\n\
        If your on-chain wallet is empty, the command pauses and prints a\n\
        deposit address; it proceeds once your deposit confirms.",
    help_template = HELP_TEMPLATE,
)]
pub struct UnilateralExitArgs {
    /// Path to the encrypted `channel_manager` backup file.
    #[arg(long)]
    channel_manager: Option<PathBuf>,

    #[arg(
        long = "monitor",
        help = "Path to an encrypted `channel_monitors <name>` backup \
                file.\nPass once per file."
    )]
    monitors: Vec<PathBuf>,

    #[arg(
        long,
        help = "Path to the `password_encrypted_root_seed` backup file.\n\
                An alternative to --root-seed / --root-seed-path."
    )]
    password_encrypted_root_seed: Option<PathBuf>,

    #[arg(
        long,
        help = "The backup password for \
                --password-encrypted-root-seed.\n[env: LEXE_BACKUP_PASSWORD]"
    )]
    backup_password: Option<String>,

    #[arg(
        long,
        help = "The Esplora URL to sync against.\n\
                Multiple URLs can be specified, separated with commas.\n\
                [default: blockstream.info and mempool.space]"
    )]
    esplora_urls: Option<String>,

    /// Skip cooperative close; immediately force close all channels.
    #[arg(long, conflicts_with = "coop_close_only")]
    force_close_only: bool,

    #[arg(
        long,
        help = "Only cooperatively close, avoiding the force close\n\
                timelock. Errors out if the LSP won't cooperate."
    )]
    coop_close_only: bool,

    /// Drain all recovered funds to this address once they've confirmed.
    #[arg(long)]
    sweep_address: Option<bitcoin::Address<NetworkUnchecked>>,

    #[arg(
        long,
        help = "The working dir for the recovery node.\n\
                [default: <LEXE_DATA_DIR>/<env>/<user_pk>/recovery]"
    )]
    recovery_dir: Option<PathBuf>,
}

impl UnilateralExitArgs {
    /// Resolve the root seed from --password-encrypted-root-seed or the
    /// standard credential args.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_root_seed(
        &self,
        env_config: &WalletEnvConfig,
        lexe_data_dir: &Option<PathBuf>,
        credentials_from_cli: bool,
        client_credentials: Option<String>,
        client_credentials_path: Option<PathBuf>,
        root_seed: Option<String>,
        root_seed_path: Option<PathBuf>,
    ) -> anyhow::Result<lexe_common::root_seed::RootSeed> {
        if let Some(path) = &self.password_encrypted_root_seed {
            let blob = std::fs::read(path).with_context(|| {
                format!("Failed to read {}", path.display())
            })?;
            let mut backup_password = self.backup_password.clone();
            backup_password.or_env_mut("LEXE_BACKUP_PASSWORD")?;
            let backup_password = backup_password.context(
                "--password-encrypted-root-seed requires the backup password \
                 via --backup-password / $LEXE_BACKUP_PASSWORD",
            )?;
            return lexe_common::root_seed::RootSeed::password_decrypt(
                &backup_password,
                blob,
            )
            .context("Failed to decrypt root seed; is the password correct?");
        }

        let credentials = helpers::resolve_credentials(
            env_config,
            lexe_data_dir,
            credentials_from_cli,
            client_credentials,
            client_credentials_path,
            root_seed,
            root_seed_path,
        )?;
        let Credentials::RootSeed(root_seed) = credentials else {
            return Err(anyhow!(
                "unilateral-exit requires a root seed. Use --root-seed / \
                 --root-seed-path or --password-encrypted-root-seed."
            ));
        };
        lexe_common::root_seed::RootSeed::try_from(root_seed.as_bytes())
            .context("Invalid root seed")
    }

    pub(crate) async fn run(
        self,
        env_config: WalletEnvConfig,
        root_seed: lexe_common::root_seed::RootSeed,
        lexe_data_dir: PathBuf,
    ) -> anyhow::Result<()> {
        let network = env_config.wallet_env.network;

        let esplora_urls = match self.esplora_urls {
            Some(urls) => urls.split(',').map(str::to_owned).collect(),
            None => match network {
                Network::Mainnet => vec![
                    constants::MAINNET_PUBLIC_BLOCKSTREAM_ESPLORA.to_owned(),
                    constants::MAINNET_PUBLIC_MEMPOOL_ESPLORA.to_owned(),
                ],
                Network::Testnet3 => vec![
                    constants::TESTNET3_PUBLIC_BLOCKSTREAM_ESPLORA.to_owned(),
                ],
                _ =>
                    return Err(anyhow!(
                        "--esplora-urls is required on {network}"
                    )),
            },
        };

        let close_mode = if self.force_close_only {
            CloseMode::ForceOnly
        } else {
            let deploy_env = env_config.wallet_env.deploy_env;
            let dev_lsp = Self::dev_lsp_from_env()?;
            let lsp = LexeLsp::for_env(deploy_env, dev_lsp)
                .context("Pass --force-close-only to proceed without one.")?;
            match self.coop_close_only {
                true => CloseMode::CoopOnly(lsp),
                false => CloseMode::CoopThenForce(lsp),
            }
        };

        // Default to a dir under the user's namespace in the Lexe data dir,
        // mirroring LexeWallet's internal layout.
        let recovery_dir = match self.recovery_dir {
            Some(dir) => dir,
            None => {
                let env_db_config = WalletEnvDbConfig::new(
                    env_config.wallet_env,
                    lexe_data_dir,
                );
                let user_db_config = WalletUserDbConfig::new(
                    env_db_config,
                    root_seed.derive_user_pk().into(),
                );
                user_db_config.user_db_dir().join("recovery")
            }
        };

        let (printer_task, progress_tx) = Self::spawn_progress_printer();
        printer_task.detach();

        let args = RecoveryArgs {
            channel_manager_path: self.channel_manager,
            close_mode,
            esplora_urls,
            monitor_paths: self.monitors,
            network,
            progress_tx: Some(progress_tx),
            recovery_dir: recovery_dir.clone(),
            root_seed,
            sweep_address: self.sweep_address,
            sync_interval: if network == Network::Regtest {
                Duration::from_secs(1)
            } else {
                // Public Esplora instances have very strict rate limits.
                Duration::from_secs(900)
            },
        };

        let shutdown = NotifyOnce::new();
        lexe_tokio::task::spawn_ctrlc_shutdown(&shutdown).detach();

        let recovery_dir_display = recovery_dir.display();
        println!("Starting recovery. Recovery dir: {recovery_dir_display}");
        println!(
            "This may run for a long time; it is safe to stop with Ctrl+C and \
             re-run with the same arguments."
        );

        // The provided backup files are stale once `init` imports them, so
        // print a copy-pasteable command to delete them once it returns.
        let any_backups_provided = args.channel_manager_path.is_some()
            || !args.monitor_paths.is_empty();
        let rm_backups_cmd = {
            let quoted_paths = args
                .channel_manager_path
                .iter()
                .chain(args.monitor_paths.iter())
                .map(|path| format!("'{}'", path.display()))
                .collect::<Vec<_>>()
                .join(" ");
            format!("rm {quoted_paths}")
        };

        let (mut node, sync_ctx, run_ctx) =
            RecoveryNode::init(args, shutdown).await?;
        if any_backups_provided {
            println!(
                "Backup files imported. The recovery dir now holds the \
                 authoritative channel state, so the downloaded backup files \
                 are stale and can be deleted:"
            );
            println!();
            println!("    {rm_backups_cmd}");
            println!();
        }

        node.sync(sync_ctx).await?;
        let report = node.run(run_ctx).await?;

        println!("Recovery complete.");
        println!("  Channels tracked:      {}", report.num_monitors);
        println!("  Coop closed this run:  {}", report.coop_closed);
        println!("  Force closed this run: {}", report.force_closed);
        let balance_sats = report.onchain_balance.sats_u64();
        println!("  On-chain balance:      {balance_sats} sats");
        if let Some(sweep_info) = report.sweep_info {
            let sweep_sats = sweep_info.amount.sats_u64();
            println!("  Swept {sweep_sats} sats to {}", sweep_info.address);
            println!("  Sweep txid: {}", sweep_info.txid);
        }
        anyhow::Ok(())
    }

    /// Spawns a task which prints [`RecoveryProgress`] updates.
    fn spawn_progress_printer()
    -> (LxTask<()>, watch::Sender<Option<RecoveryProgress>>) {
        let (progress_tx, mut progress_rx) =
            watch::channel::<Option<RecoveryProgress>>(None);
        let task = LxTask::spawn("recovery-progress-printer", async move {
            // A multi-day contest delay looks like a hang, so explain
            // timelocked funds the first time we see them.
            let mut explained_timelocks = false;
            let mut last_anchor: Option<AnchorFundsNeeded> = None;
            while progress_rx.changed().await.is_ok() {
                let Some(progress) = progress_rx.borrow_and_update().clone()
                else {
                    continue;
                };

                if progress.awaiting_anchor_funds != last_anchor {
                    match &progress.awaiting_anchor_funds {
                        Some(anchor) => {
                            let confirmed_sats =
                                anchor.confirmed_balance.sats_u64();
                            let needed_sats = anchor.needed_deposit.sats_u64();
                            let suggested_sats =
                                anchor.suggested_deposit.sats_u64();
                            let deposit_address = &anchor.deposit_address;
                            let uri = Bip321Uri {
                                onchain: vec![
                                    deposit_address.as_unchecked().clone(),
                                ],
                                amount: Amount::try_from_sats_u64(
                                    suggested_sats,
                                )
                                .ok(),
                                ..Default::default()
                            };
                            println!();
                            println!(
                                "ACTION REQUIRED: Force closing your channels \
                                 requires confirmed on-chain funds to pay \
                                 fees, but the recovery wallet can't provide \
                                 them. Deposit at least \
                                 ~{needed_sats} sats (suggested: \
                                 ~{suggested_sats} sats, since fee-bumps \
                                 escalate until your deposit confirms) to:"
                            );
                            println!();
                            println!("    {deposit_address}");
                            let _ =
                                helpers::encode_and_print_qr(&uri.to_string());
                            println!(
                                "The recovery wallet holds ~{confirmed_sats} \
                                 confirmed sats, already counted toward the \
                                 amounts above. The exit will proceed \
                                 automatically once your deposit confirms. \
                                 If fees rise and more is needed, an updated \
                                 notice will print. Any unspent deposit is \
                                 recovered along with your channel funds."
                            );
                        }
                        None => println!(
                            "Deposit confirmed; recovery is proceeding."
                        ),
                    }
                    last_anchor = progress.awaiting_anchor_funds.clone();
                }

                if !explained_timelocks
                    && let Some(unlock_height) = progress.unlock_height
                {
                    explained_timelocks = true;
                    let blocks_remaining =
                        unlock_height.saturating_sub(progress.best_height);
                    let days = f64::from(blocks_remaining) / 144.0;
                    println!();
                    println!(
                        "Funds from force-closed channels are timelocked \
                         on-chain until block {unlock_height} \
                         (~{blocks_remaining} blocks, roughly {days:.1} \
                         days). The exit proceeds automatically once the \
                         timelock expires; it is safe to stop with Ctrl+C \
                         and re-run later."
                    );
                }

                let timelock_part = match progress.unlock_height {
                    Some(unlock_height) => format!(
                        ", ~{} blocks until timelocks expire",
                        unlock_height.saturating_sub(progress.best_height),
                    ),
                    None => String::new(),
                };
                let best_height = progress.best_height;
                let num_channels = progress.num_channels;
                let claimable_sats = progress.claimable_balance.sats_u64();
                let pending_sats = progress.pending_balance.sats_u64();
                let confirmed_sats = progress.confirmed_balance.sats_u64();
                println!(
                    "Progress @ block {best_height}: {num_channels} channels \
                     open, {claimable_sats} sats claimable, {pending_sats} \
                     sats pending, {confirmed_sats} sats \
                     confirmed{timelock_part}"
                );
                if let Some(sweep) = &progress.sweep_progress {
                    let txid = &sweep.info.txid;
                    let confs = sweep.confs;
                    let confs_required = sweep.confs_required;
                    if progress.is_swept() {
                        println!("External sweep {txid} confirmed.");
                    } else {
                        println!(
                            "External sweep {txid}: {confs}/{confs_required} \
                             confs"
                        );
                    }
                }
            }
        });
        (task, progress_tx)
    }

    /// The local dev cluster's LSP, as configured by the monorepo `.env`.
    fn dev_lsp_from_env() -> anyhow::Result<Option<LexeLsp>> {
        let (Ok(root_seed), Ok(p2p_addr)) = (
            env::var("DEV_LSP_ROOT_SEED"),
            env::var("LSP_PUBLIC_P2P_ADDR"),
        ) else {
            return Ok(None);
        };
        let node_pk = lexe_common::root_seed::RootSeed::from_str(&root_seed)
            .context("Invalid DEV_LSP_ROOT_SEED")?
            .derive_node_pk();
        // This is the LSP's listen address, e.g. `[::]:9735`, so dial its port
        // on localhost.
        let port = SocketAddr::from_str(&p2p_addr)
            .context("Invalid LSP_PUBLIC_P2P_ADDR")?
            .port();
        let addr = LxSocketAddress::TcpIpv4 {
            ip: Ipv4Addr::LOCALHOST,
            port,
        };
        Ok(Some(LexeLsp {
            node_pk,
            addrs: vec![addr],
        }))
    }
}

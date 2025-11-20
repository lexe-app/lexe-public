use std::{ops::Deref, slice, sync::Arc};

use anyhow::{Context, ensure};
use axum::extract::State;
use common::{
    api::{
        models::{
            BroadcastedTx, BroadcastedTxInfo, SignMsgRequest, SignMsgResponse,
            VerifyMsgRequest, VerifyMsgResponse,
        },
        revocable_clients::{
            CreateRevocableClientRequest, CreateRevocableClientResponse,
            GetRevocableClients, RevocableClients, UpdateClientRequest,
            UpdateClientResponse,
        },
    },
    constants::{self},
    ln::{amount::Amount, channel::LxUserChannelId},
    rng::SysRng,
};
use gdrive::gvfs::GvfsRootName;
use lexe_api::{
    def::NodeBackendApi,
    error::NodeApiError,
    models::{
        command::{
            BackupInfo, CloseChannelRequest, CreateOfferRequest,
            CreateOfferResponse, GDriveStatus, GetAddressResponse,
            GetNewPayments, GetUpdatedPayments, ListChannelsResponse,
            LxPaymentIdStruct, NodeInfo, OpenChannelRequest,
            OpenChannelResponse, PayInvoiceRequest, PayInvoiceResponse,
            PayOfferRequest, PayOfferResponse, PayOnchainRequest,
            PayOnchainResponse, PaymentAddress, PaymentCreatedIndexes,
            PreflightCloseChannelRequest, PreflightCloseChannelResponse,
            PreflightOpenChannelRequest, PreflightOpenChannelResponse,
            PreflightPayInvoiceRequest, PreflightPayInvoiceResponse,
            PreflightPayOfferRequest, PreflightPayOfferResponse,
            PreflightPayOnchainRequest, PreflightPayOnchainResponse,
            SetupGDrive, UpdatePaymentAddress, UpdatePaymentNote,
        },
        nwc::{
            CreateNwcWalletRequest, CreateNwcWalletResponse,
            GetNwcWalletsParams, ListNwcWalletResponse, NostrPkStruct,
            UpdateNwcWalletRequest, UpdateNwcWalletResponse,
        },
    },
    server::{LxJson, extract::LxQuery},
    types::{
        Empty,
        payments::{
            BasicPaymentV1, MaybeBasicPaymentV2, VecBasicPaymentV1,
            VecBasicPaymentV2,
        },
        username::UsernameStruct,
    },
    vfs::{self, Vfs, VfsDirectory},
};
use lexe_ln::p2p;
use lexe_tokio::task::MaybeLxTask;
use tracing::warn;

use super::RouterState;
use crate::{gdrive_setup, nwc::NwcWallet};

pub(super) async fn node_info(
    State(state): State<Arc<RouterState>>,
) -> LxJson<NodeInfo> {
    let channels = state.channel_manager.list_channels();
    LxJson(lexe_ln::command::node_info(
        state.version.clone(),
        state.measurement,
        state.user_pk,
        &state.channel_manager,
        &state.peer_manager,
        &state.wallet,
        &state.chain_monitor,
        &channels,
        state.lsp_info.lsp_fees(),
    ))
}

pub(super) async fn list_channels(
    State(state): State<Arc<RouterState>>,
) -> Result<LxJson<ListChannelsResponse>, NodeApiError> {
    let channels = state.channel_manager.list_channels();
    lexe_ln::command::list_channels(
        &state.network_graph,
        &state.chain_monitor,
        channels,
    )
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn sign_message(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<SignMsgRequest>,
) -> LxJson<SignMsgResponse> {
    let sig = state.keys_manager.sign_message(&req.msg);
    LxJson(SignMsgResponse { sig })
}

pub(super) async fn verify_message(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<VerifyMsgRequest>,
) -> LxJson<VerifyMsgResponse> {
    let VerifyMsgRequest { msg, sig, pk } = &req;
    let is_valid = state.keys_manager.verify_message(msg, sig, pk);
    LxJson(VerifyMsgResponse { is_valid })
}

pub(super) async fn open_channel(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<OpenChannelRequest>,
) -> Result<LxJson<OpenChannelResponse>, NodeApiError> {
    let RouterState {
        lsp_info,
        peer_manager,
        channel_manager,
        channel_events_bus,
        wallet,
        ..
    } = &*state;

    ensure_channel_value_in_range(&req.value)?;

    let user_channel_id = LxUserChannelId::from_rng(&mut SysRng::new());
    let lsp_node_pk = &lsp_info.node_pk;
    let lsp_addrs = slice::from_ref(&lsp_info.private_p2p_addr);

    // Callback ensures we're connected to the LSP.
    let eph_tasks_tx = state.eph_tasks_tx.clone();
    let ensure_lsp_connected = || async move {
        let maybe_task = p2p::connect_peer_if_necessary(
            peer_manager,
            lsp_node_pk,
            lsp_addrs,
        )
        .await
        .context("Could not connect to Lexe LSP")?;

        if let MaybeLxTask(Some(task)) = maybe_task
            && eph_tasks_tx.try_send(task).is_err()
        {
            warn!("(open_channel) Couldn't send task");
        }

        Ok(())
    };

    // Open the channel and wait for `ChannelPending`.
    let is_jit_channel = false;
    lexe_ln::command::open_channel(
        channel_manager,
        channel_events_bus,
        wallet,
        ensure_lsp_connected,
        user_channel_id,
        req.value,
        lsp_node_pk,
        *state.config,
        is_jit_channel,
    )
    .await
    .context("Failed to open channel")
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn preflight_open_channel(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PreflightOpenChannelRequest>,
) -> Result<LxJson<PreflightOpenChannelResponse>, NodeApiError> {
    ensure_channel_value_in_range(&req.value)?;

    lexe_ln::command::preflight_open_channel(&state.wallet, req)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

/// Check the `open_channel` value against the min/max bounds early so it fails
/// in preflight with a good error message.
fn ensure_channel_value_in_range(value: &Amount) -> Result<(), NodeApiError> {
    let value = value.sats_u64();

    let min_value = constants::LSP_USERNODE_CHANNEL_MIN_FUNDING_SATS as u64;
    if value < min_value {
        return Err(NodeApiError::command(format!(
            "Channel value is below limit ({min_value} sats)"
        )));
    }

    let max_value = constants::CHANNEL_MAX_FUNDING_SATS as u64;
    if value > max_value {
        return Err(NodeApiError::command(format!(
            "Channel value is above limit ({max_value} sats)"
        )));
    }

    Ok(())
}

pub(super) async fn close_channel(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<CloseChannelRequest>,
) -> Result<LxJson<Empty>, NodeApiError> {
    let RouterState {
        lsp_info,
        peer_manager,
        channel_manager,
        channel_events_bus,
        ..
    } = &*state;

    // During a cooperative channel close, we want to guarantee that we're
    // connected to the LSP. Proactively reconnect if necessary.
    let lsp_node_pk = &lsp_info.node_pk;
    let lsp_addrs = slice::from_ref(&lsp_info.private_p2p_addr);
    let eph_tasks_tx = state.eph_tasks_tx.clone();
    let ensure_lsp_connected = |node_pk| async move {
        ensure!(&node_pk == lsp_node_pk, "Can only connect to the Lexe LSP");

        let maybe_task = p2p::connect_peer_if_necessary(
            peer_manager,
            lsp_node_pk,
            lsp_addrs,
        )
        .await
        .context("Could not connect to Lexe LSP")?;

        if let MaybeLxTask(Some(task)) = maybe_task
            && eph_tasks_tx.try_send(task).is_err()
        {
            warn!("(close_channel) Failed to send connection task");
        }

        Ok(())
    };

    lexe_ln::command::close_channel(
        channel_manager,
        channel_events_bus,
        ensure_lsp_connected,
        req,
    )
    .await
    .context("Failed to close channel")
    .map(|()| LxJson(Empty {}))
    .map_err(NodeApiError::command)
}

pub(super) async fn preflight_close_channel(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PreflightCloseChannelRequest>,
) -> Result<LxJson<PreflightCloseChannelResponse>, NodeApiError> {
    lexe_ln::command::preflight_close_channel(
        &state.channel_manager,
        &state.chain_monitor,
        &state.fee_estimates,
        req,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn pay_invoice(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PayInvoiceRequest>,
) -> Result<LxJson<PayInvoiceResponse>, NodeApiError> {
    lexe_ln::command::pay_invoice(
        req,
        &state.router,
        &state.channel_manager,
        &state.payments_manager,
        &state.network_graph,
        &state.chain_monitor,
        state.lsp_info.lsp_fees(),
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn preflight_pay_invoice(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PreflightPayInvoiceRequest>,
) -> Result<LxJson<PreflightPayInvoiceResponse>, NodeApiError> {
    lexe_ln::command::preflight_pay_invoice(
        req,
        &state.router,
        &state.channel_manager,
        &state.payments_manager,
        &state.network_graph,
        &state.chain_monitor,
        state.lsp_info.lsp_fees(),
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn create_offer(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<CreateOfferRequest>,
) -> Result<LxJson<CreateOfferResponse>, NodeApiError> {
    lexe_ln::command::create_offer(req, &state.channel_manager)
        .await
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn pay_offer(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PayOfferRequest>,
) -> Result<LxJson<PayOfferResponse>, NodeApiError> {
    lexe_ln::command::pay_offer(
        req,
        &state.router,
        &state.channel_manager,
        &state.payments_manager,
        &state.chain_monitor,
        &state.network_graph,
        state.lsp_info.lsp_fees(),
        &state.lsp_info.node_pk,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn preflight_pay_offer(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PreflightPayOfferRequest>,
) -> Result<LxJson<PreflightPayOfferResponse>, NodeApiError> {
    lexe_ln::command::preflight_pay_offer(
        req,
        &state.router,
        &state.channel_manager,
        &state.payments_manager,
        &state.chain_monitor,
        &state.network_graph,
        state.lsp_info.lsp_fees(),
        &state.lsp_info.node_pk,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn pay_onchain(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PayOnchainRequest>,
) -> Result<LxJson<PayOnchainResponse>, NodeApiError> {
    let response = lexe_ln::command::pay_onchain(
        req,
        state.network,
        &state.wallet,
        &state.tx_broadcaster,
        &state.payments_manager,
    )
    .await
    .map_err(NodeApiError::command)?;

    Ok(LxJson(response))
}

pub(super) async fn preflight_pay_onchain(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PreflightPayOnchainRequest>,
) -> Result<LxJson<PreflightPayOnchainResponse>, NodeApiError> {
    lexe_ln::command::preflight_pay_onchain(req, &state.wallet, state.network)
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn get_address(
    State(state): State<Arc<RouterState>>,
) -> LxJson<GetAddressResponse> {
    let addr = state.wallet.get_address().into_unchecked();
    LxJson(GetAddressResponse { addr })
}

pub(super) async fn get_payments_by_indexes(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PaymentCreatedIndexes>,
) -> Result<LxJson<VecBasicPaymentV1>, NodeApiError> {
    let ids = req.indexes.into_iter().map(|index| index.id).collect();
    let payments = state
        .persister
        .read_payments_by_ids(ids)
        .await
        .map(|p| p.into_iter().map(BasicPaymentV1::from).collect())
        .map_err(NodeApiError::command)?;
    Ok(LxJson(VecBasicPaymentV1 { payments }))
}

pub(super) async fn get_new_payments(
    State(state): State<Arc<RouterState>>,
    LxQuery(req): LxQuery<GetNewPayments>,
) -> Result<LxJson<VecBasicPaymentV1>, NodeApiError> {
    let payments = state
        .persister
        .read_new_payments(req)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(VecBasicPaymentV1 { payments }))
}

pub(super) async fn get_updated_payments(
    State(state): State<Arc<RouterState>>,
    LxQuery(req): LxQuery<GetUpdatedPayments>,
) -> Result<LxJson<VecBasicPaymentV2>, NodeApiError> {
    let payments = state
        .persister
        .get_updated_basic_payments(req)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(VecBasicPaymentV2 { payments }))
}

pub(super) async fn get_payment_by_id(
    State(state): State<Arc<RouterState>>,
    LxQuery(req): LxQuery<LxPaymentIdStruct>,
) -> Result<LxJson<MaybeBasicPaymentV2>, NodeApiError> {
    let maybe_payment = state
        .persister
        .read_payment_by_id(req.id)
        .await
        .map_err(NodeApiError::command)?;

    Ok(LxJson(MaybeBasicPaymentV2 { maybe_payment }))
}

pub(super) async fn update_payment_note(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<UpdatePaymentNote>,
) -> Result<LxJson<Empty>, NodeApiError> {
    state
        .payments_manager
        .update_payment_note(req)
        .await
        .map_err(NodeApiError::command)?;

    Ok(LxJson(Empty {}))
}

pub(super) async fn get_revocable_clients(
    State(state): State<Arc<RouterState>>,
    LxQuery(req): LxQuery<GetRevocableClients>,
) -> Result<LxJson<RevocableClients>, NodeApiError> {
    let locked_revocable_clients = state.revocable_clients.read().unwrap();

    let revocable_clients = if req.valid_only {
        let clients = locked_revocable_clients
            .iter_valid()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        RevocableClients { clients }
    } else {
        locked_revocable_clients.clone()
    };

    Ok(LxJson(revocable_clients))
}

pub(super) async fn create_revocable_client(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<CreateRevocableClientRequest>,
) -> Result<LxJson<CreateRevocableClientResponse>, NodeApiError> {
    lexe_ln::command::create_revocable_client(
        state.user_pk,
        &state.persister,
        state.eph_ca_cert_der.deref().clone(),
        &state.rev_ca_cert,
        &state.revocable_clients,
        req,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn update_revocable_client(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<UpdateClientRequest>,
) -> Result<LxJson<UpdateClientResponse>, NodeApiError> {
    lexe_ln::command::update_revocable_client(
        &state.persister,
        &state.revocable_clients,
        req,
    )
    .await
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn list_broadcasted_txs(
    State(state): State<Arc<RouterState>>,
) -> Result<LxJson<Vec<BroadcastedTxInfo>>, NodeApiError> {
    let directory = VfsDirectory {
        dirname: vfs::BROADCASTED_TXS_DIR.into(),
    };
    let broadcasted_txs = state
        .persister
        .read_dir_json::<BroadcastedTx>(&directory)
        .await
        .map_err(NodeApiError::command)?;

    let txs = broadcasted_txs
        .into_iter()
        .map(|(_, broadcasted_tx)| {
            let confirmation_block_height = state
                .wallet
                .get_tx_details(broadcasted_tx.txid)
                .and_then(|details| {
                    details.chain_position.confirmation_height_upper_bound()
                });

            let tx_info = BroadcastedTxInfo::from_broadcasted_tx(
                broadcasted_tx,
                state.network,
                confirmation_block_height,
            )?;
            Ok::<_, anyhow::Error>(tx_info)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(NodeApiError::command)?;
    Ok(LxJson(txs))
}

pub(super) async fn backup_info(
    State(state): State<Arc<RouterState>>,
) -> Result<LxJson<BackupInfo>, NodeApiError> {
    let gdrive_status = state.gdrive_status.lock().await.clone();

    let backup_info = BackupInfo { gdrive_status };

    Ok(LxJson(backup_info))
}

pub(super) async fn setup_gdrive(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<SetupGDrive>,
) -> Result<LxJson<Empty>, NodeApiError> {
    // Hold the lock during setup to prevent concurrent GDrive setups, and avoid
    // setting up GDrive if already set up.
    let mut locked_gdrive_status = state.gdrive_status.lock().await;

    if *locked_gdrive_status == GDriveStatus::Ok {
        return Ok(LxJson(Empty {}));
    }

    let oauth =
        state.gdrive_oauth_config.as_ref().as_ref().ok_or_else(|| {
            NodeApiError::command("OAuthConfig required in staging/prod")
        })?;

    let mut rng = SysRng::new();
    let gdrive_client = gdrive::ReqwestClient::new();
    let backend_api = state.persister.backend_api();
    let authenticator = state.persister.authenticator();
    let vfs_master_key = state.persister.vfs_master_key();
    let credentials_result =
        gdrive_setup::exchange_code_and_persist_credentials(
            &mut rng,
            backend_api,
            &gdrive_client,
            oauth,
            &req.google_auth_code,
            authenticator,
            vfs_master_key,
        )
        .await;

    let credentials = match credentials_result {
        Ok(credentials) => credentials,
        Err(err) => {
            *locked_gdrive_status = GDriveStatus::Error(format!("{err:#}"));
            return Err(NodeApiError::command(err));
        }
    };

    let gvfs_root_name = GvfsRootName {
        deploy_env: state.deploy_env,
        network: state.network,
        use_sgx: cfg!(target_env = "sgx"),
        user_pk: state.user_pk,
    };

    let init_result = gdrive_setup::setup_gvfs_and_persist_seed(
        Some(req.encrypted_seed),
        gvfs_root_name,
        backend_api,
        &mut rng,
        authenticator,
        credentials,
        vfs_master_key,
    )
    .await;

    match init_result {
        Ok(_) => *locked_gdrive_status = GDriveStatus::Ok,
        Err(err) => {
            *locked_gdrive_status = GDriveStatus::Error(format!("{err:#}"));
            return Err(NodeApiError::command(err));
        }
    }

    Ok(LxJson(Empty {}))
}

pub(super) async fn get_payment_address(
    State(state): State<Arc<RouterState>>,
) -> Result<LxJson<PaymentAddress>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let payment_address = state
        .persister
        .backend_api()
        .get_payment_address(token)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(payment_address))
}

pub(super) async fn update_payment_address(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<UsernameStruct>,
) -> Result<LxJson<PaymentAddress>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let bitcoin_address = format!("{}@lexe.app", req.username.inner());
    let description = format!("Pay to {}", bitcoin_address);

    let offer_req = CreateOfferRequest {
        expiry_secs: None,
        amount: None,
        description: Some(description),
        max_quantity: None,
        issuer: Some(bitcoin_address),
    };

    let offer =
        lexe_ln::command::create_offer(offer_req, &state.channel_manager)
            .await
            .map_err(NodeApiError::command)?;

    let req = UpdatePaymentAddress {
        username: req.username,
        offer: offer.offer,
    };
    let payment_address = state
        .persister
        .backend_api()
        .update_payment_address(req, token)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(payment_address))
}

/// List all NWC wallets for the current user.
///
/// Returns wallet info without the connection string URI.
pub(super) async fn list_nwc_wallets(
    State(state): State<Arc<RouterState>>,
) -> Result<LxJson<ListNwcWalletResponse>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let params = GetNwcWalletsParams {
        wallet_nostr_pk: None,
    };

    let vec_nwc_wallet = state
        .persister
        .backend_api()
        .get_nwc_wallets(params, token)
        .await
        .map_err(NodeApiError::command)?;

    // Decrypt each wallet cyphertext to get the label and expose the client
    // info.
    let mut wallets = Vec::new();
    for wallet in vec_nwc_wallet.nwc_wallets {
        let client_data =
            NwcWallet::from_db(state.persister.vfs_master_key(), wallet);
        if let Ok(connection) = client_data {
            wallets.push(connection.to_nwc_client_info());
        }
    }

    Ok(LxJson(ListNwcWalletResponse { wallets }))
}

/// Create a new NWC wallet.
///
/// Generates keys and returns the connection string.
pub(super) async fn create_nwc_wallet(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<CreateNwcWalletRequest>,
) -> Result<LxJson<CreateNwcWalletResponse>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let nwc_wallet = NwcWallet::new(req.label);

    let db_nwc_wallet = state
        .persister
        .backend_api()
        .upsert_nwc_wallet(
            nwc_wallet
                .to_req(&mut SysRng::new(), state.persister.vfs_master_key()),
            token,
        )
        .await
        .map_err(NodeApiError::command)?;

    let connection_string = nwc_wallet
        .connection_string()
        .ok_or_else(|| {
            NodeApiError::command(
                "Connection string should be present for new wallet",
            )
        })?
        .to_string();

    Ok(LxJson(CreateNwcWalletResponse {
        wallet_nostr_pk: db_nwc_wallet.wallet_nostr_pk,
        label: nwc_wallet.label().to_string(),
        connection_string,
    }))
}

/// Update an existing NWC wallet's label.
pub(super) async fn update_nwc_wallet(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<UpdateNwcWalletRequest>,
) -> Result<LxJson<UpdateNwcWalletResponse>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let params = GetNwcWalletsParams {
        wallet_nostr_pk: Some(req.wallet_nostr_pk),
    };

    // First fetch the wallet from the DB as we need to decrypt the ciphertext
    // to update the label and then encrypt it back and persist it.
    let vec_nwc_wallet = state
        .persister
        .backend_api()
        .get_nwc_wallets(params, token.clone())
        .await
        .map_err(NodeApiError::command)?;

    let db_nwc_wallet = vec_nwc_wallet
        .nwc_wallets
        .into_iter()
        .last()
        .ok_or_else(|| NodeApiError::command("NWC wallet not found"))?;

    let mut nwc_wallet =
        NwcWallet::from_db(state.persister.vfs_master_key(), db_nwc_wallet)
            .map_err(NodeApiError::command)?;

    nwc_wallet.update_label(req.label);

    let db_nwc_wallet = state
        .persister
        .backend_api()
        .upsert_nwc_wallet(
            nwc_wallet
                .to_req(&mut SysRng::new(), state.persister.vfs_master_key()),
            token,
        )
        .await
        .map_err(NodeApiError::command)?;

    // We build the nwc wallet from the already decrypted ciphertext and
    // update the timestamps from the backend response.
    let mut wallet_info = nwc_wallet.to_nwc_client_info();
    wallet_info.updated_at = db_nwc_wallet.updated_at;
    wallet_info.created_at = db_nwc_wallet.created_at;

    Ok(LxJson(UpdateNwcWalletResponse { wallet_info }))
}

/// Delete an NWC client.
pub(super) async fn delete_nwc_wallet(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<NostrPkStruct>,
) -> Result<LxJson<Empty>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;
    state
        .persister
        .backend_api()
        .delete_nwc_wallet(req, token)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(Empty {}))
}

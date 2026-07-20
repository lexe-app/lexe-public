use std::{ops::Deref, slice, sync::Arc};

use anyhow::{Context, ensure};
use axum::extract::State;
use gdrive::gvfs::GvfsRootName;
use lexe_api::{
    def::NodeBackendApi,
    error::NodeApiError,
    models::{
        command::{
            BackupInfo, CloseChannelPreflightRequest,
            CloseChannelPreflightResponse, CloseChannelRequest,
            CreateOfferRequest, CreateOfferResponse, DebugInfo, GDriveStatus,
            GetHumanBitcoinAddressResponse, GetNewPayments,
            GetNextUnusedAddressResponse, GetUpdatedPayments,
            HumanBitcoinAddressV1, ListChannelsResponse, NodeInfo,
            OpenChannelPreflightRequest, OpenChannelPreflightResponse,
            OpenChannelRequest, OpenChannelResponse,
            PayInvoicePreflightRequest, PayInvoicePreflightResponse,
            PayInvoiceRequest, PayInvoiceResponse, PayOfferPreflightRequest,
            PayOfferPreflightResponse, PayOfferRequest, PayOfferResponse,
            PayOnchainPreflightRequest, PayOnchainPreflightResponse,
            PayOnchainRequest, PayOnchainResponse, PaymentCreatedIndexes,
            PaymentIdStruct, SetupGDrive, UpdatePersonalNote,
            UpsertCustomHumanBitcoinAddress, UpsertHumanBitcoinAddressResponse,
        },
        nwc::{
            CreateNwcClientRequest, CreateNwcClientResponse, GetNwcClients,
            ListNwcClientResponse, NostrPkStruct, UpdateNwcClientRequest,
            UpdateNwcClientResponse,
        },
    },
    revocable_clients::{
        RevocableClients,
        models::{
            CreateRevocableClientRequest, CreateRevocableClientResponse,
            ListRevocableClients, UpdateClientRequest, UpdateClientResponse,
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
use lexe_common::{
    api::{
        auth::BearerAuthToken,
        models::{
            BroadcastedTx, BroadcastedTxInfo, SignMsgRequest, SignMsgResponse,
            VerifyMsgRequest, VerifyMsgResponse,
        },
    },
    constants::{self},
    ln::amount::Amount,
};
use lexe_crypto::rng::SysRng;
use lexe_ln::p2p;
use lexe_tokio::task::MaybeLxTask;
use tracing::warn;

use super::RouterState;
use crate::{gdrive_setup, nwc::NwcClient};

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

pub(super) async fn debug_info(
    State(state): State<Arc<RouterState>>,
) -> LxJson<DebugInfo> {
    let utxo_counts = state.wallet.get_utxo_counts();
    let pending_monitor_updates = state
        .chain_monitor
        .list_pending_monitor_updates()
        .values()
        .map(|v| v.len())
        .sum::<usize>();

    LxJson(DebugInfo {
        descriptors: state.descriptors.clone(),
        legacy_descriptors: state.legacy_descriptors.clone(),
        num_utxos: utxo_counts.total,
        num_confirmed_utxos: utxo_counts.confirmed,
        num_unconfirmed_utxos: utxo_counts.unconfirmed,
        pending_monitor_updates: Some(pending_monitor_updates),
    })
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

    let OpenChannelRequest {
        user_channel_id,
        value,
    } = req;

    ensure_channel_value_in_range(&value)?;

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
    let push_amount = None;
    lexe_ln::command::open_channel(
        channel_manager,
        channel_events_bus,
        wallet,
        ensure_lsp_connected,
        user_channel_id,
        value,
        lsp_node_pk,
        (*state.config).clone(),
        is_jit_channel,
        push_amount,
    )
    .await
    .context("Failed to open channel")
    .map(LxJson)
    .map_err(NodeApiError::command)
}

pub(super) async fn open_channel_preflight(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<OpenChannelPreflightRequest>,
) -> Result<LxJson<OpenChannelPreflightResponse>, NodeApiError> {
    ensure_channel_value_in_range(&req.value)?;

    lexe_ln::command::open_channel_preflight(&state.wallet, req)
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

pub(super) async fn close_channel_preflight(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<CloseChannelPreflightRequest>,
) -> Result<LxJson<CloseChannelPreflightResponse>, NodeApiError> {
    lexe_ln::command::close_channel_preflight(
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

pub(super) async fn pay_invoice_preflight(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PayInvoicePreflightRequest>,
) -> Result<LxJson<PayInvoicePreflightResponse>, NodeApiError> {
    lexe_ln::command::pay_invoice_preflight(
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

pub(super) async fn pay_offer_preflight(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PayOfferPreflightRequest>,
) -> Result<LxJson<PayOfferPreflightResponse>, NodeApiError> {
    lexe_ln::command::pay_offer_preflight(
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

pub(super) async fn pay_onchain_preflight(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<PayOnchainPreflightRequest>,
) -> Result<LxJson<PayOnchainPreflightResponse>, NodeApiError> {
    lexe_ln::command::pay_onchain_preflight(req, &state.wallet, state.network)
        .map(LxJson)
        .map_err(NodeApiError::command)
}

pub(super) async fn get_next_unused_address(
    State(state): State<Arc<RouterState>>,
) -> LxJson<GetNextUnusedAddressResponse> {
    let addr = state.wallet.get_next_unused_address().into_unchecked();
    LxJson(GetNextUnusedAddressResponse { addr })
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
    LxQuery(req): LxQuery<PaymentIdStruct>,
) -> Result<LxJson<MaybeBasicPaymentV2>, NodeApiError> {
    let maybe_payment = state
        .persister
        .read_payment_by_id(req.id)
        .await
        .map_err(NodeApiError::command)?;

    Ok(LxJson(MaybeBasicPaymentV2 { maybe_payment }))
}

pub(super) async fn update_personal_note(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<UpdatePersonalNote>,
) -> Result<LxJson<Empty>, NodeApiError> {
    state
        .payments_manager
        .update_personal_note(req)
        .await
        .map_err(NodeApiError::command)?;

    Ok(LxJson(Empty {}))
}

pub(super) async fn list_revocable_clients(
    State(state): State<Arc<RouterState>>,
    LxQuery(req): LxQuery<ListRevocableClients>,
) -> Result<LxJson<RevocableClients>, NodeApiError> {
    let locked_revocable_clients = state.revocable_clients.0.read().unwrap();

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
    let gateway_proxy_token = state
        .persister
        .mint_long_lived_gateway_proxy_token()
        .await
        .map_err(NodeApiError::command)?;

    lexe_ln::command::create_revocable_client(
        state.user_pk,
        Some(gateway_proxy_token),
        &state.persister,
        state.eph_ca_cert_der.deref().clone(),
        &state.rev_ca_cert,
        &state.revocable_clients.0,
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
        &state.revocable_clients.0,
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

pub(super) async fn get_human_bitcoin_address(
    State(state): State<Arc<RouterState>>,
) -> Result<LxJson<GetHumanBitcoinAddressResponse>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let hba = state
        .persister
        .backend_api()
        .get_human_bitcoin_address(token)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(hba))
}

pub(super) async fn get_human_bitcoin_address_v1(
    state: State<Arc<RouterState>>,
) -> Result<LxJson<HumanBitcoinAddressV1>, NodeApiError> {
    let resp = get_human_bitcoin_address(state).await?;
    Ok(LxJson(HumanBitcoinAddressV1::from(resp.0)))
}

pub(super) async fn upsert_custom_human_bitcoin_address(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<UsernameStruct>,
) -> Result<LxJson<UpsertHumanBitcoinAddressResponse>, NodeApiError> {
    check_hba_claim_min_balance(&state)?;
    let (token, req) = build_upsert_custom_hba_request(&state, req).await?;
    let resp = state
        .persister
        .backend_api()
        .upsert_custom_human_bitcoin_address(req, token)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(resp))
}

/// Require a minimum total wallet balance to claim or update a custom HBA.
/// Deters mass-claiming of usernames.
fn check_hba_claim_min_balance(
    state: &RouterState,
) -> Result<(), NodeApiError> {
    let channels = state.channel_manager.list_channels();
    let (lightning_balance, _num_usable_channels) =
        lexe_ln::balance::all_channel_balances(
            &state.chain_monitor,
            &channels,
            state.lsp_info.lsp_fees(),
        );
    let onchain_balance = Amount::try_from(state.wallet.get_balance().total())
        .map_err(NodeApiError::command)?;
    let balance = lightning_balance.total() + onchain_balance;

    let min_balance =
        Amount::from_sats_u32(constants::HBA_CLAIM_MIN_BALANCE_SATS);
    if balance < min_balance {
        return Err(NodeApiError::command(format!(
            "Claiming a custom Human Bitcoin Address requires a total \
             wallet balance of at least {min_balance} sats"
        )));
    }
    Ok(())
}

async fn build_upsert_custom_hba_request(
    state: &RouterState,
    req: UsernameStruct,
) -> Result<(BearerAuthToken, UpsertCustomHumanBitcoinAddress), NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let offer_req = lexe_ln::command::hba_offer_request(&req.username)
        .context("Failed to build HBA offer request")
        .map_err(NodeApiError::command)?;
    let offer =
        lexe_ln::command::create_offer(offer_req, &state.channel_manager)
            .await
            .map_err(NodeApiError::command)?;
    let offer_id = offer.offer.id();

    let req = UpsertCustomHumanBitcoinAddress {
        username: req.username,
        offer: offer.offer,
    };

    // Cache the fresh HBA offer ID so receives to the new offer are labeled
    // with `PaymentKind::HumanBitcoinAddress`.
    state.hba_offer_ids.write().unwrap().insert(offer_id);

    Ok((token, req))
}

/// List all NWC clients for the current user.
///
/// Returns client info without the connection string URI.
pub(super) async fn list_nwc_clients(
    State(state): State<Arc<RouterState>>,
) -> Result<LxJson<ListNwcClientResponse>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let params = GetNwcClients {
        client_nostr_pk: None,
    };

    let vec_nwc_client = state
        .persister
        .backend_api()
        .get_nwc_clients(params, token)
        .await
        .map_err(NodeApiError::command)?;

    // Decrypt each ciphertext to get the label and expose the client info.
    let clients = vec_nwc_client
        .nwc_clients
        .into_iter()
        .filter_map(|client| {
            NwcClient::decrypt(state.persister.vfs_master_key(), client)
                .map(|c| c.to_nwc_client_info())
                .inspect_err(|e| warn!("Failed to decrypt NWC client: {e:#}"))
                .ok()
        })
        .collect();

    Ok(LxJson(ListNwcClientResponse { clients }))
}

/// Create a new NWC client.
///
/// Generates keys and returns the connection string.
pub(super) async fn create_nwc_client(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<CreateNwcClientRequest>,
) -> Result<LxJson<CreateNwcClientResponse>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let mut rng = SysRng::new();
    let (nwc_client, connection_string) =
        NwcClient::new(&mut rng, state.deploy_env, req.label);
    let fields = nwc_client.encrypt(&mut rng, state.persister.vfs_master_key());

    let db_nwc_client = state
        .persister
        .backend_api()
        .upsert_nwc_client(fields, token)
        .await
        .map_err(NodeApiError::command)?;

    Ok(LxJson(CreateNwcClientResponse {
        wallet_nostr_pk: db_nwc_client.fields.wallet_nostr_pk,
        client_nostr_pk: db_nwc_client.fields.client_nostr_pk,
        label: nwc_client.label().to_string(),
        connection_string,
    }))
}

/// Update an existing NWC client's label.
pub(super) async fn update_nwc_client(
    State(state): State<Arc<RouterState>>,
    LxJson(req): LxJson<UpdateNwcClientRequest>,
) -> Result<LxJson<UpdateNwcClientResponse>, NodeApiError> {
    let token = state
        .persister
        .get_token()
        .await
        .map_err(NodeApiError::command)?;

    let params = GetNwcClients {
        client_nostr_pk: Some(req.client_nostr_pk),
    };

    // Fetch the client from the DB, decrypt, update label, re-encrypt, persist.
    //
    // NOTE: This read-modify-write pattern has a TOCTTOU race if any other
    // endpoint also updates NwcClients concurrently - e.g. if `nwc_request`
    // updates budget spent, a concurrent label update here could overwrite it.
    //
    // TODO(max): Add synchronization for NwcClient updates - could use a
    // write-through cache similar to PaymentsManager.
    let mut db_nwc_clients = state
        .persister
        .backend_api()
        .get_nwc_clients(params, token.clone())
        .await
        .map_err(NodeApiError::command)?
        .nwc_clients;

    let db_nwc_client = match db_nwc_clients.len() {
        0 => Err(NodeApiError::command("NWC client not found")),
        1 => Ok(db_nwc_clients.pop().unwrap()),
        _ => Err(NodeApiError::command("More than one NWC client found")),
    }?;

    let mut nwc_client =
        NwcClient::decrypt(state.persister.vfs_master_key(), db_nwc_client)
            .map_err(NodeApiError::command)?;

    nwc_client.update_label(req.label);
    let mut rng = SysRng::new();
    let fields = nwc_client.encrypt(&mut rng, state.persister.vfs_master_key());

    let db_nwc_client = state
        .persister
        .backend_api()
        .upsert_nwc_client(fields, token)
        .await
        .map_err(NodeApiError::command)?;

    // We build the client info from the plaintext and update the timestamps
    // from the backend response.
    let mut client_info = nwc_client.to_nwc_client_info();
    client_info.updated_at = db_nwc_client.updated_at;
    client_info.created_at = db_nwc_client.created_at;

    Ok(LxJson(UpdateNwcClientResponse { client_info }))
}

/// Delete an NWC client.
pub(super) async fn delete_nwc_client(
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
        .delete_nwc_client(req, token)
        .await
        .map_err(NodeApiError::command)?;
    Ok(LxJson(Empty {}))
}

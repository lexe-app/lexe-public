//! Client budget handling.
//!
//! Budgets live inside the [`PaymentsManager`] and are updated under the
//! payments lock as each budgeted payment is created, retried, and finalized.
//! This module holds budget logic which runs outside of the `PaymentsManager`.
//!
//! [`PaymentsManager`]: crate::payments::manager::PaymentsManager

use std::{collections::HashMap, sync::RwLock};

use anyhow::Context;
use lexe_api::{
    budget::{Budget, ClientBudgets, ClientPk},
    def::FiatRatesBackendApi,
    models::command::GetFiatRatesRequest,
    revocable_clients::RevocableClients,
    types::payments::{PaymentCreatedIndex, PaymentDirection},
};
use lexe_common::api::fiat_rates::IsoCurrencyCode;

use crate::{payments::PaymentWithMetadata, traits::LexePaymentsPersister};

/// Make sure each revocable client has a budget. Each client should have a
/// budget, but clients predating budgets may be legitimately missing one.
///
/// Finalizing a client's payment fails unless its budget tracks it, so we also
/// register each of the client's pending payments in its new budget. We don't
/// have historical fiat rates, so they're valued at the current rate.
//
// compat: Added in node-v0.10.9.
pub async fn create_missing_at_startup<PS: LexePaymentsPersister>(
    budgets: &mut ClientBudgets,
    revocable_clients: &RwLock<RevocableClients>,
    pending_payments: &[PaymentWithMetadata],
    fiat_currency: IsoCurrencyCode,
    fiat_rates_api: &impl FiatRatesBackendApi,
    persister: &PS,
) -> anyhow::Result<()> {
    let clients_without_budget = revocable_clients
        .read()
        .unwrap()
        .clients
        .keys()
        .map(|pk| ClientPk::Revocable(*pk))
        .filter(|client_pk| budgets.get(client_pk).is_none())
        .collect::<Vec<_>>();
    if clients_without_budget.is_empty() {
        return Ok(());
    }

    // Group the pending outbound payments by client.
    let mut outbound_pending_with_clients = HashMap::<_, Vec<_>>::new();
    for pwm in pending_payments {
        if pwm.payment.direction() != PaymentDirection::Outbound {
            continue;
        }
        let client_pk = match pwm.metadata.client_pk {
            Some(pk) => ClientPk::Revocable(pk),
            None => continue,
        };
        outbound_pending_with_clients
            .entry(client_pk)
            .or_default()
            .push(pwm);
    }

    let req = GetFiatRatesRequest {
        currency_codes: Some(vec![fiat_currency]),
    };
    let fiat_rates = fiat_rates_api
        .get_fiat_rates(req)
        .await
        .context("Could not fetch fiat rates")?;

    for client_pk in clients_without_budget {
        let budget_policy = None;
        let mut budget = Budget::new(budget_policy, fiat_currency)?;

        // Register the client's pending payments
        if let Some(pending) = outbound_pending_with_clients.get(&client_pk) {
            for pwm in pending {
                let id = pwm.payment.id();
                let created_at = pwm.payment.created_at().expect(
                    "All persisted payments must have a created_at set",
                );
                let index = PaymentCreatedIndex { created_at, id };
                let amount = pwm.payment.amount().with_context(|| {
                    format!("Outbound pending payment {id} has no amount")
                })?;
                let total = amount.saturating_add(pwm.payment.fee());
                budget.register_new_payment(index, total, &fiat_rates)?;
            }
        }

        let persisted_budget =
            ClientBudgets::persist(&**persister, client_pk, budget)
                .await
                .context("Could not persist new budget")?;
        budgets.commit(persisted_budget);
    }

    Ok(())
}

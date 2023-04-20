use common::api::error::{NodeApiError, NodeErrorKind};
use common::api::qs::{GetNewPayments, GetPaymentsByIds, UpdatePaymentNote};
use common::ln::payments::BasicPayment;

use crate::alias::NodePaymentsManagerType;
use crate::persister::NodePersister;

pub(super) async fn get_payments_by_ids(
    req: GetPaymentsByIds,
    persister: NodePersister,
) -> Result<Vec<BasicPayment>, NodeApiError> {
    persister
        .read_payments_by_ids(req)
        .await
        .map_err(|e| NodeApiError {
            kind: NodeErrorKind::Command,
            msg: format!("Could not read `BasicPayment`s by ids: {e:#}"),
        })
}

pub(super) async fn get_new_payments(
    req: GetNewPayments,
    persister: NodePersister,
) -> Result<Vec<BasicPayment>, NodeApiError> {
    persister
        .read_new_payments(req)
        .await
        .map_err(|e| NodeApiError {
            kind: NodeErrorKind::Command,
            msg: format!("Could not read new `BasicPayment`s: {e:#}"),
        })
}

pub(super) async fn update_payment_note(
    update: UpdatePaymentNote,
    payments_manager: NodePaymentsManagerType,
) -> Result<(), NodeApiError> {
    payments_manager
        .update_payment_note(update)
        .await
        .map_err(|e| NodeApiError {
            kind: NodeErrorKind::Command,
            msg: format!("Could not update payment note: {e:#}"),
        })
}

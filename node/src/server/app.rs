use common::{
    api::{
        qs::{GetNewPayments, GetPaymentsByIds, UpdatePaymentNote},
        Empty,
    },
    ln::payments::BasicPayment,
};

use crate::{alias::NodePaymentsManagerType, persister::NodePersister};

pub(super) async fn get_payments_by_ids(
    req: GetPaymentsByIds,
    persister: NodePersister,
) -> anyhow::Result<Vec<BasicPayment>> {
    persister.read_payments_by_ids(req).await
}

pub(super) async fn get_new_payments(
    req: GetNewPayments,
    persister: NodePersister,
) -> anyhow::Result<Vec<BasicPayment>> {
    persister.read_new_payments(req).await
}

pub(super) async fn update_payment_note(
    update: UpdatePaymentNote,
    payments_manager: NodePaymentsManagerType,
) -> anyhow::Result<Empty> {
    payments_manager.update_payment_note(update).await?;
    Ok(Empty {})
}

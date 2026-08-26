//! Bounded, tenant-scoped reads over immutable delegation-use receipts.

use tokio_postgres::Client;

use super::model::{
    row_to_use_receipt, DelegationUseError, DelegationUseReceiptCursor, DelegationUseReceiptPage,
    MAX_RECEIPT_PAGE, USE_RECEIPT_COLUMNS,
};

pub(super) async fn list_use_receipts(
    client: &Client,
    tenant_id: &str,
    repository_id: &str,
    delegation_id: &str,
    after: Option<&DelegationUseReceiptCursor>,
    requested_limit: i64,
) -> Result<DelegationUseReceiptPage, DelegationUseError> {
    let effective_limit = requested_limit.clamp(1, MAX_RECEIPT_PAGE);
    let after_receipt_id = after
        .map(|cursor| {
            i64::try_from(cursor.receipt_id).map_err(|_| DelegationUseError::InvalidCursor)
        })
        .transpose()?;
    let rows = client
        .query(
            &format!(
                "SELECT {USE_RECEIPT_COLUMNS} FROM delegation_use_receipts \
                 WHERE tenant_id = $1 AND repository_id = $2 AND delegation_id = $3 \
                   AND ($4::bigint IS NULL OR receipt_id > $4) \
                 ORDER BY receipt_id ASC LIMIT $5"
            ),
            &[
                &tenant_id,
                &repository_id,
                &delegation_id,
                &after_receipt_id,
                &(effective_limit + 1),
            ],
        )
        .await?;
    let has_next = rows.len() > effective_limit as usize;
    let entries = rows
        .iter()
        .take(effective_limit as usize)
        .map(row_to_use_receipt)
        .collect::<Result<Vec<_>, _>>()?;
    let next_after = has_next.then(|| {
        entries.last().map(|entry| DelegationUseReceiptCursor {
            receipt_id: entry.receipt_id,
        })
    });

    Ok(DelegationUseReceiptPage {
        entries,
        effective_limit,
        next_after: next_after.flatten(),
    })
}

//! SQLite scalar functions used by deterministic Lodestar reads.

use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;

use crate::error::Result;

pub(super) fn register(connection: &Connection) -> Result<()> {
    connection.create_scalar_function(
        "effective_weight",
        4,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |context| {
            let base: f64 = context.get(0)?;
            let half_life: f64 = context.get(1)?;
            let confirmed_at: i64 = context.get(2)?;
            let now: i64 = context.get(3)?;
            Ok(crate::decay::effective_weight(
                base,
                half_life,
                confirmed_at,
                now,
            ))
        },
    )?;
    Ok(())
}

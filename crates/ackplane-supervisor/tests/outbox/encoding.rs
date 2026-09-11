use super::*;

// Decoding used to discard unknown fields, so replay changed the stored evidence.
// Refuse the encoding without rewriting either pending or acknowledged records.
#[test]
fn outbox_reads_refuse_lossy_wire_decoding_without_changing_evidence() {
    for archived in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("outbox.db");
        let outbox = SupervisorOutbox::open(&path, registration(), session()).unwrap();
        let first = outbox
            .enqueue_next(lifecycle(v1::SupervisorWorkerState::Started, None))
            .unwrap();
        let second = outbox
            .enqueue_next(lifecycle(v1::SupervisorWorkerState::Terminated, None))
            .unwrap();
        if archived {
            outbox.acknowledge_through(2).unwrap();
        }
        let positions = outbox.positions().unwrap();
        drop(outbox);
        let mut changed = second.frame.encode_to_vec();
        changed.extend_from_slice(&[0x98, 0x06, 0x01]);
        assert_eq!(
            v1::NodeFrame::decode(changed.as_slice()).unwrap(),
            second.frame
        );
        let table = if archived {
            "acknowledged_lifecycle_receipts"
        } else {
            "outbound_frames"
        };
        let database = rusqlite::Connection::open(&path).unwrap();
        database
            .execute(
                &format!("UPDATE {table} SET frame = ?1 WHERE sequence = 2"),
                [&changed],
            )
            .unwrap();
        for read_only in [false, true] {
            let reader = if read_only {
                SupervisorOutbox::open_read_only(&path, registration(), session())
            } else {
                SupervisorOutbox::open(&path, registration(), session())
            }
            .unwrap();
            let result = if archived {
                reader.acknowledged_lifecycle_receipts(0, 32)
            } else {
                reader.pending(32)
            };
            let error = result.expect_err("unknown protobuf fields must not disappear on replay");
            assert!(matches!(
                error,
                OutboxError::UnsupportedStoredEncoding { sequence: 2 }
            ));
            assert!(error.to_string().contains("wire bytes"), "{error}");
            assert!(error.to_string().contains("sequence 2"), "{error}");
            assert_eq!(reader.positions().unwrap(), positions);
            let rows: Vec<(u64, Vec<u8>)> = database
                .prepare(&format!(
                    "SELECT sequence, frame FROM {table} ORDER BY sequence"
                ))
                .unwrap()
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                rows,
                vec![(1, first.frame.encode_to_vec()), (2, changed.clone())]
            );
        }
    }
}

### Added

- Added a durable local `NodeFrame` outbox to `ackplane-supervisor`. It stores outbound frames before a future NodeSync sender can transmit them, enforces positive contiguous local sequencing with a persisted high-water mark, replays identical queued frames, refuses changed-frame collisions, exposes bounded ordered pending reads, and prunes only frames acknowledged through a supplied sequence. The outbox does not open a connection or send frames itself.

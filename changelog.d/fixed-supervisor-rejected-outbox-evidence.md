- **Rejected supervisor evidence is retained:** a permanent Ackplane frame
  refusal now stops delivery with a typed sequence/reason diagnostic instead
  of deleting the refused frame and falsely advancing acknowledgement. The
  rejected frame and queued tail survive outbox reopen for operator recovery;
  only genuinely accepted frames are pruned. Retryable refusals still retain
  the queue and reconnect. This does not reconstruct previously discarded
  evidence or add automatic recovery.

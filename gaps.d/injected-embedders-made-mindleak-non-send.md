- **Injected embedders made `MindLeak` non-`Send`.** — Commit `5d52877` added
  `Box<dyn TextEmbedder>` without the thread-safety contract required when the
  maintenance runtime moves `MindLeak` into `std::thread::spawn`. — High impact:
  the workspace build and strict clippy were red. — Resolved Jul 2026 by making
  `TextEmbedder: Send + Sync` and adding compile-time and unit regression
  assertions that `MindLeak: Send` (Lodestar task `task:e0548f57556a`).

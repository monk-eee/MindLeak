- Fixed a Windows offline-test teardown race in archive verification: temporary
  ZIP fixtures are removed only after their file handle closes, including when
  enumeration fails. The original archive permission assertions remain intact;
  deterministic lifecycle regressions cover successful reads and read errors.

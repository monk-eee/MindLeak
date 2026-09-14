- Enrollment tests bound exact native credential cleanup to ten seconds in an
  owned child process. Stalled cleanup is terminated and reaped, failure keeps
  the original recovery metadata, and a zero-exit child must explicitly confirm
  that cleanup ran. Production credential storage and the documented upstream
  GNOME Keyring limitation are unchanged.

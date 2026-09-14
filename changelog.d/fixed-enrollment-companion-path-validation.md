- Fixed enrollment accepting Unix state directories whose resolved companion
  socket path exceeds the platform limit. Provisioning and recovery now validate
  the endpoint before accessing credentials and explain that a shorter state
  directory is required. Symlink aliases cannot bypass the check. Existing keys
  and receipts remain unchanged when an unusable path is refused.

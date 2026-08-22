- **Bridge now exposes tenant-scoped Evidence and conformance detail
  resources.** Each lookup requires the repository, task, and record ID and
  returns `404` outside the current tenant scope, supporting explicit review
  drill-down without exposing a cross-tenant record handle.- **Bridge now exposes explicit tenant-scoped Evidence and conformance detail
  resources.** A detail lookup is scoped by repository, task, and record ID,
  so the Evidence Board can drill into one typed record without turning IDs
  into cross-tenant discovery handles.

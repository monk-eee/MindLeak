- **Ackplane pgvector migration diagnostics:** failures while applying the
  projection embeddings schema now name the pgvector installation and migration
  role prerequisites instead of reporting only `db error`. The operator message
  does not include database connection strings, SQL parameters, or server error
  details; the original driver error remains available as the error cause.

-- `KnowledgeStore::active_page` is a tenant/repository-scoped keyset read.
-- Its partial index excludes retired statements because they never appear on
-- the Bridge's active knowledge route, preserving the exact page order
-- without growing the read path with historical rows.
CREATE INDEX IF NOT EXISTS knowledge_active_page_order
    ON knowledge (tenant_id, repository_id, confirmed_at DESC, knowledge_id ASC)
    WHERE retired_at IS NULL;

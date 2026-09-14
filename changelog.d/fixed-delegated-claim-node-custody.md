- Fixed enrolled peers being able to mutate another node's delegated lease by
  signing its owner ID. Grants now persist authenticated node custody; owner
  mutations and context compilation require that node as well as the session.
  Migration 67 leaves older node custody unknown instead of inventing authority.
  Resolve parked claims and drain active workers before upgrading. Bridge
  stranded-claim recovery now requires an explicit destination `node_id`.

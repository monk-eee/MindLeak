use ackplane_client::{
    companion::{wire::Operation, NodeClient},
    ClientError,
};
use ackplane_protocol::knowledge_auth::{knowledge_signing_bytes, KnowledgeOperation};
use ackplane_protocol::v1::{
    KnowledgeAuthentication, ListMissingProjectionEmbeddingsRequest,
    ListMissingProjectionEmbeddingsResult, ProjectionEmbeddingSource,
    PublishProjectionEmbeddingRequest, PublishProjectionEmbeddingResult,
};
use ed25519_dalek::{Signer, SigningKey};
use prost::Message;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tonic::Code;

use crate::projection::tests::require_test_database;
use crate::signing_keys::{self, KeyRevocation};
use crate::test_support::{test_pool, unique_id};

mod authentication;
#[path = "../../../../ackplane-node/tests/support/companion.rs"]
mod companion;
mod paging;
mod publication;
mod recall;
mod replay;
mod response_bounds;
mod support;

use companion::TestCompanion;
use support::{Fixture, TestServer};

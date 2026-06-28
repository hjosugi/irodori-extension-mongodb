# Native Source

The initial source snapshot was copied from `db/mongo.rs` in the desktop app.

Source SHA-256: `7dd5843e37ef37c30d857aceebc37c86f90d0c1d38abac2174f68db25157c2b2`.


This directory is a migration staging area for `irodori.mongodb`. The active native
ABI shim lives in `src/lib.rs`; engine-specific connect/query/metadata behavior
should move here as the connector runtime contract is wired into the desktop app.

Engine status from `knowledge/engines.json`: `verified`.

# MongoDB Connector

Adds MongoDB connectivity as an installable connector extension.

This connector is listed in the public Irodori extension marketplace.

## Connector

- Extension ID: `irodori.mongodb`
- Engine ID: `mongodb`
- Wire: `mongodb`
- Default port: `27017`
- Native ABI: `irodori.connector.native.v1`
- Driver linked: `true`

A desktop adapter source snapshot is staged in `native/source/` from `db/mongo.rs`.

Connector metadata lives in `connector.config.json` and `irodori.extension.json`.
The Rust code keeps native ABI exports in `src/lib.rs`, shared buffer/JSON helpers in `src/abi.rs`, and MongoDB behavior in `src/driver.rs`.

## Connection Metadata

- Endpoint modes: `hostPort`, `connectionString`
- Transport modes: `direct`, `sshTunnel`, `socks5Proxy`, `httpConnectProxy`, `proxyChain`
- TLS supported: `true`
- Custom driver options: `true`

| Auth method | Label | Secret purposes |
|---|---|---|
| `none` | No authentication | none |
| `connectionString` | Connection string / DSN | none |
| `scram` | SCRAM user/password | `password` |
| `mongodbX509` | X.509 client certificate | `privateKey`, `privateKeyPassphrase` |
| `awsIam` | AWS IAM | `token` |
| `kerberos` | Kerberos / GSSAPI | `token` |
| `ldap` | LDAP user/password | `password` |
| `oidc` | OIDC / workload identity | `token` |
| `customDriverOptions` | Custom driver options | `password`, `token`, `privateKey`, `privateKeyPassphrase` |

## ABI Calls

The driver handles these JSON requests today:

| Method | Response |
|---|---|
| `health` / `ping` | Connector health, engine id, ABI version, and driver link status. |
| `describe` / `capabilities` | Embedded manifest and connector config. |
| `manifest` | Raw `irodori.extension.json`. |
| `config` | Raw `connector.config.json`. |
| `connect` | Opens a MongoDB client and validates it with `buildInfo`. |
| `query` | Reads documents from a collection name or JSON `{ collection, filter }`. |
| `metadata` | Samples collections and indexes for metadata. |
| `close` | Removes the cached native connection. |

## Development


Generated extension repositories share `../target` across sibling repositories so Rust dependencies are compiled once per checkout. DuckDB and MotherDuck are driver-linked by default; set `IRODORI_CONNECTOR_LINK_DUCKDB=0` only when you need metadata-only DuckDB-compatible scaffolds.


```sh
make check
make build
```

Release packages place platform-specific native artifacts under `dist/native`.

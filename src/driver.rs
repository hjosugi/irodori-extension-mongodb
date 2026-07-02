use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use futures_util::TryStreamExt;
use mongodb::bson::{doc, to_document, Bson, Document};
use mongodb::Client;
use serde_json::{json, Map, Value};
use tokio::runtime::Runtime;

use crate::abi::{self, IrodoriConnectorBuffer};
use crate::{ABI_VERSION, CONFIG_JSON, DRIVER_LINKED, ENGINE, MANIFEST_JSON};

static CONNECTIONS: OnceLock<Mutex<HashMap<String, MongoConnection>>> = OnceLock::new();
static RUNTIME: OnceLock<Runtime> = OnceLock::new();

#[derive(Clone)]
struct MongoConnection {
    client: Client,
    config: MongoConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MongoConfig {
    uri: String,
    database: String,
    redaction_values: Vec<String>,
}

type QueryRows = Vec<Vec<Value>>;
type QueryOutput = (Vec<String>, QueryRows, bool);

fn connections() -> &'static Mutex<HashMap<String, MongoConnection>> {
    CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn runtime() -> Result<&'static Runtime, String> {
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let runtime = Runtime::new().map_err(|err| format!("create tokio runtime failed: {err}"))?;
    let _ = RUNTIME.set(runtime);
    RUNTIME
        .get()
        .ok_or_else(|| "create tokio runtime failed.".to_string())
}

pub fn call_json(request: IrodoriConnectorBuffer) -> IrodoriConnectorBuffer {
    let request = match abi::parse_request(request) {
        Ok(request) => request,
        Err(response) => return response,
    };
    let method = match abi::request_method(request.as_ref()) {
        Ok(method) => method,
        Err(response) => return response,
    };

    match method {
        "health" | "ping" => abi::ok(Map::from_iter([
            ("engine".to_string(), Value::String(ENGINE.to_string())),
            ("abiVersion".to_string(), json!(ABI_VERSION)),
            ("driverLinked".to_string(), Value::Bool(DRIVER_LINKED)),
        ])),
        "describe" | "capabilities" => abi::ok(Map::from_iter([
            ("engine".to_string(), Value::String(ENGINE.to_string())),
            ("abiVersion".to_string(), json!(ABI_VERSION)),
            ("driverLinked".to_string(), Value::Bool(DRIVER_LINKED)),
            (
                "manifest".to_string(),
                serde_json::from_str(MANIFEST_JSON).unwrap_or(Value::Null),
            ),
            (
                "config".to_string(),
                serde_json::from_str(CONFIG_JSON).unwrap_or(Value::Null),
            ),
        ])),
        "manifest" => abi::owned_buffer(MANIFEST_JSON.to_string()),
        "config" => abi::owned_buffer(CONFIG_JSON.to_string()),
        "connect" => connect(request.as_ref().expect("connect has request")),
        "query" => query(request.as_ref().expect("query has request")),
        "metadata" => metadata(request.as_ref().expect("metadata has request")),
        "close" => close(request.as_ref().expect("close has request")),
        other => abi::error(
            "connector.unknownMethod",
            format!("unknown connector method: {other}"),
        ),
    }
}

fn connect(request: &Value) -> IrodoriConnectorBuffer {
    let connection_id = abi::connection_id(Some(request));
    let config = match MongoConfig::from_request(request) {
        Ok(config) => config,
        Err(err) => return abi::error("connector.invalidRequest", err),
    };
    let connection =
        match runtime().and_then(|runtime| runtime.block_on(MongoConnection::new(config))) {
            Ok(connection) => connection,
            Err(err) => return abi::error("connector.connectFailed", err),
        };
    let version = match runtime().and_then(|runtime| runtime.block_on(load_version(&connection))) {
        Ok(version) => version,
        Err(err) => return abi::error("connector.connectFailed", connection.config.redact(&err)),
    };
    let mut guard = match connections().lock() {
        Ok(guard) => guard,
        Err(_) => {
            return abi::error(
                "connector.statePoisoned",
                "Connector connection state is poisoned.",
            )
        }
    };
    let mut response = Map::from_iter([
        ("engine".to_string(), Value::String(ENGINE.to_string())),
        (
            "connectionId".to_string(),
            Value::String(connection_id.clone()),
        ),
        ("driverLinked".to_string(), Value::Bool(DRIVER_LINKED)),
        (
            "database".to_string(),
            Value::String(connection.config.database.clone()),
        ),
    ]);
    if let Some(version) = version {
        response.insert("serverVersion".to_string(), Value::String(version));
    }
    guard.insert(connection_id, connection);
    abi::ok(response)
}

fn query(request: &Value) -> IrodoriConnectorBuffer {
    let connection_id = abi::connection_id(Some(request));
    let Some(input) = abi::string_field(request, "query")
        .or_else(|| abi::string_field(request, "sql"))
        .or_else(|| abi::string_field(request, "statement"))
        .or_else(|| abi::string_field(request, "collection"))
    else {
        return abi::error(
            "connector.invalidRequest",
            "query requires a string query, sql, statement, or collection field.",
        );
    };
    let connection = match connection(&connection_id) {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    match runtime()
        .and_then(|runtime| runtime.block_on(run_query(&connection, input, abi::max_rows(request))))
    {
        Ok((columns, rows, truncated)) => abi::ok(Map::from_iter([
            ("connectionId".to_string(), Value::String(connection_id)),
            (
                "columns".to_string(),
                Value::Array(columns.into_iter().map(Value::String).collect()),
            ),
            (
                "rows".to_string(),
                Value::Array(rows.into_iter().map(Value::Array).collect()),
            ),
            ("truncated".to_string(), Value::Bool(truncated)),
        ])),
        Err(err) => abi::error("connector.queryFailed", connection.config.redact(&err)),
    }
}

fn metadata(request: &Value) -> IrodoriConnectorBuffer {
    let connection_id = abi::connection_id(Some(request));
    let connection = match connection(&connection_id) {
        Ok(connection) => connection,
        Err(response) => return response,
    };
    match runtime().and_then(|runtime| runtime.block_on(load_metadata(&connection))) {
        Ok(metadata) => abi::ok(Map::from_iter([
            ("connectionId".to_string(), Value::String(connection_id)),
            ("metadata".to_string(), metadata),
        ])),
        Err(err) => abi::error("connector.metadataFailed", connection.config.redact(&err)),
    }
}

fn close(request: &Value) -> IrodoriConnectorBuffer {
    let connection_id = abi::connection_id(Some(request));
    let mut guard = match connections().lock() {
        Ok(guard) => guard,
        Err(_) => {
            return abi::error(
                "connector.statePoisoned",
                "Connector connection state is poisoned.",
            )
        }
    };
    let existed = guard.remove(&connection_id).is_some();
    abi::ok(Map::from_iter([
        ("connectionId".to_string(), Value::String(connection_id)),
        ("closed".to_string(), Value::Bool(existed)),
    ]))
}

impl MongoConnection {
    async fn new(config: MongoConfig) -> Result<Self, String> {
        let client = Client::with_uri_str(&config.uri)
            .await
            .map_err(|err| format!("MongoDB connect failed: {err}"))?;
        Ok(Self { client, config })
    }
}

impl MongoConfig {
    fn from_request(request: &Value) -> Result<Self, String> {
        let uri = option_string(request, &["connectionString", "url", "dsn"])
            .unwrap_or_else(|| build_uri(request));
        let database = option_string(request, &["database", "db"])
            .or_else(|| database_from_uri(&uri))
            .unwrap_or_else(|| "test".to_string());
        let mut redaction_values = Vec::new();
        push_sensitive(
            &mut redaction_values,
            option_string(request, &["password"]).as_deref(),
        );
        push_sensitive(
            &mut redaction_values,
            option_string(request, &["token"]).as_deref(),
        );
        collect_url_auth(&uri, &mut redaction_values);
        Ok(Self {
            uri,
            database,
            redaction_values,
        })
    }

    fn redact(&self, message: &str) -> String {
        self.redaction_values.iter().fold(
            message.replace(&self.uri, "<mongodb-uri>"),
            |message, secret| {
                if secret.is_empty() {
                    message
                } else {
                    message.replace(secret, "****")
                }
            },
        )
    }
}

async fn load_version(connection: &MongoConnection) -> Result<Option<String>, String> {
    let response = connection
        .client
        .database(&connection.config.database)
        .run_command(doc! { "buildInfo": 1 })
        .await
        .map_err(|err| format!("MongoDB buildInfo failed: {err}"))?;
    Ok(response
        .get_str("version")
        .ok()
        .map(|version| format!("MongoDB {version}")))
}

async fn run_query(
    connection: &MongoConnection,
    input: &str,
    cap: usize,
) -> Result<QueryOutput, String> {
    let (collection_name, filter) = parse_input(input)?;
    let collection = connection
        .client
        .database(&connection.config.database)
        .collection::<Document>(&collection_name);
    let mut cursor = collection
        .find(filter)
        .await
        .map_err(|err| format!("MongoDB query failed: {err}"))?;
    let mut docs = Vec::new();
    let mut truncated = false;
    while let Some(doc) = cursor
        .try_next()
        .await
        .map_err(|err| format!("MongoDB query failed: {err}"))?
    {
        if docs.len() >= cap {
            truncated = true;
            break;
        }
        docs.push(doc);
    }
    Ok(documents_to_output(docs, truncated))
}

async fn load_metadata(connection: &MongoConnection) -> Result<Value, String> {
    let database = connection.client.database(&connection.config.database);
    let names = database
        .list_collection_names()
        .await
        .map_err(|err| format!("MongoDB collection metadata failed: {err}"))?;
    let mut objects = Vec::new();
    for name in names {
        let collection = database.collection::<Document>(&name);
        let mut keys: Vec<(String, String)> = Vec::new();
        let mut cursor = collection
            .find(Document::new())
            .limit(20)
            .await
            .map_err(|err| format!("MongoDB metadata sample failed for {name}: {err}"))?;
        while let Some(doc) = cursor
            .try_next()
            .await
            .map_err(|err| format!("MongoDB metadata sample failed for {name}: {err}"))?
        {
            for (key, value) in doc.iter() {
                if !keys.iter().any(|(existing, _)| existing == key) {
                    keys.push((key.clone(), bson_type_name(value).to_string()));
                }
            }
        }
        let mut indexes = Vec::new();
        let mut index_cursor = collection
            .list_indexes()
            .await
            .map_err(|err| format!("MongoDB index metadata failed for {name}: {err}"))?;
        while let Some(index) = index_cursor
            .try_next()
            .await
            .map_err(|err| format!("MongoDB index metadata failed for {name}: {err}"))?
        {
            indexes.push(json!({
                "name": index
                    .options
                    .as_ref()
                    .and_then(|options| options.name.clone())
                    .unwrap_or_else(|| index.keys.keys().cloned().collect::<Vec<_>>().join("_")),
                "columns": index.keys.keys().cloned().collect::<Vec<_>>(),
                "unique": index.options.as_ref().and_then(|options| options.unique).unwrap_or(false)
            }));
        }
        objects.push(json!({
            "schema": connection.config.database,
            "name": name,
            "kind": "collection",
            "columns": keys
                .into_iter()
                .enumerate()
                .map(|(index, (name, data_type))| {
                    json!({
                        "name": name,
                        "dataType": data_type,
                        "nullable": true,
                        "ordinal": index + 1
                    })
                })
                .collect::<Vec<_>>(),
            "indexes": indexes,
            "primaryKey": [],
            "foreignKeys": []
        }));
    }
    Ok(json!({
        "schemas": [{
            "name": connection.config.database,
            "objects": objects
        }]
    }))
}

fn documents_to_output(docs: Vec<Document>, truncated: bool) -> QueryOutput {
    let mut columns = Vec::new();
    for doc in &docs {
        for key in doc.keys() {
            if !columns.iter().any(|column| column == key) {
                columns.push(key.clone());
            }
        }
    }
    let rows = docs
        .iter()
        .map(|doc| {
            columns
                .iter()
                .map(|key| {
                    doc.get(key)
                        .cloned()
                        .map(|value| value.into_relaxed_extjson())
                        .unwrap_or(Value::Null)
                })
                .collect()
        })
        .collect();
    (columns, rows, truncated)
}

fn parse_input(input: &str) -> Result<(String, Document), String> {
    let input = input.trim();
    if input.starts_with('{') {
        let value: Value =
            serde_json::from_str(input).map_err(|err| format!("invalid query JSON: {err}"))?;
        let collection = value
            .get("collection")
            .and_then(Value::as_str)
            .ok_or("query JSON needs a string collection field")?
            .to_string();
        let filter = match value.get("filter") {
            Some(filter) => to_document(filter).map_err(|err| format!("invalid filter: {err}"))?,
            None => Document::new(),
        };
        Ok((collection, filter))
    } else if input.is_empty() {
        Err("query needs a collection name or JSON query object.".to_string())
    } else {
        Ok((input.to_string(), Document::new()))
    }
}

fn bson_type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Array(_) => "array",
        Bson::Document(_) => "document",
        Bson::Boolean(_) => "bool",
        Bson::Null => "null",
        Bson::RegularExpression(_) => "regex",
        Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_) => "javascript",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::Timestamp(_) => "timestamp",
        Bson::Binary(_) => "binary",
        Bson::ObjectId(_) => "objectId",
        Bson::DateTime(_) => "date",
        Bson::Symbol(_) => "symbol",
        Bson::Decimal128(_) => "decimal128",
        Bson::Undefined => "undefined",
        Bson::MaxKey => "maxKey",
        Bson::MinKey => "minKey",
        Bson::DbPointer(_) => "dbPointer",
    }
}

fn build_uri(request: &Value) -> String {
    let host = option_string(request, &["host", "endpoint"]).unwrap_or_else(|| "localhost".into());
    let port = option_string(request, &["port"]).unwrap_or_else(|| "27017".into());
    let database = option_string(request, &["database", "db"]).unwrap_or_default();
    let username = option_string(request, &["user", "username"]);
    let password = option_string(request, &["password"]);
    let auth = match (username, password) {
        (Some(username), Some(password)) => format!("{username}:{password}@"),
        (Some(username), None) => format!("{username}@"),
        _ => String::new(),
    };
    format!("mongodb://{auth}{host}:{port}/{database}")
}

fn database_from_uri(uri: &str) -> Option<String> {
    let after_host = uri.split("://").nth(1)?.split('/').nth(1)?;
    let database = after_host.split('?').next().unwrap_or("").trim();
    if database.is_empty() {
        None
    } else {
        Some(database.to_string())
    }
}

fn connection(connection_id: &str) -> Result<MongoConnection, IrodoriConnectorBuffer> {
    let guard = connections().lock().map_err(|_| {
        abi::error(
            "connector.statePoisoned",
            "Connector connection state is poisoned.",
        )
    })?;
    guard.get(connection_id).cloned().ok_or_else(|| {
        abi::error(
            "connector.connectionNotFound",
            format!("no open connection: {connection_id}"),
        )
    })
}

fn request_containers(request: &Value) -> Vec<&Value> {
    [
        Some(request),
        request.get("profile"),
        request.get("options"),
        request.get("auth"),
        request.get("secrets"),
        request
            .get("profile")
            .and_then(|profile| profile.get("options")),
        request
            .get("profile")
            .and_then(|profile| profile.get("auth")),
        request
            .get("profile")
            .and_then(|profile| profile.get("secrets")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn option_string(request: &Value, fields: &[&str]) -> Option<String> {
    request_containers(request)
        .into_iter()
        .find_map(|container| {
            fields.iter().find_map(|field| {
                container
                    .get(*field)
                    .map(|value| match value {
                        Value::String(value) => value.clone(),
                        Value::Number(value) => value.to_string(),
                        Value::Bool(value) => value.to_string(),
                        _ => String::new(),
                    })
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
        })
}

fn push_sensitive(values: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        if !values.iter().any(|existing| existing == value) {
            values.push(value.to_string());
        }
    }
}

fn collect_url_auth(url: &str, values: &mut Vec<String>) {
    let Some(after_scheme) = url.split_once("://").map(|(_, rest)| rest) else {
        return;
    };
    let Some(auth) = after_scheme
        .split('/')
        .next()
        .and_then(|host| host.split('@').next())
    else {
        return;
    };
    if auth.contains(':') {
        for part in auth.split(':') {
            push_sensitive(values, Some(part));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_collection_query_json() {
        let (collection, filter) =
            parse_input(r#"{"collection":"users","filter":{"active":true}}"#).unwrap();
        assert_eq!(collection, "users");
        assert_eq!(filter.get_bool("active").ok(), Some(true));
    }

    #[test]
    fn projects_documents_to_rows() {
        let docs = vec![doc! {"a": 1, "b": "x"}, doc! {"b": "y", "c": true}];
        let (columns, rows, truncated) = documents_to_output(docs, false);
        assert_eq!(columns, vec!["a", "b", "c"]);
        assert_eq!(rows[0], vec![json!(1), json!("x"), Value::Null]);
        assert_eq!(rows[1], vec![Value::Null, json!("y"), json!(true)]);
        assert!(!truncated);
    }

    #[test]
    fn builds_uri_from_profile() {
        let request = json!({
            "profile": {
                "host": "mongo.local",
                "port": 27018,
                "database": "app",
                "user": "u",
                "password": "p"
            }
        });
        let config = MongoConfig::from_request(&request).unwrap();
        assert_eq!(config.uri, "mongodb://u:p@mongo.local:27018/app");
        assert_eq!(config.database, "app");
    }
}

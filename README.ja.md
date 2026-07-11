<!-- i18n: language-switcher -->
[English](README.md) | [日本語](README.ja.md)

# MongoDB コネクタ

MongoDB 用のネイティブ Irodori テーブルコネクタ拡張。

このクレートは、Irodori 拡張マーケットプレイスで使用されるコネクタのメタデータ、ネイティブ ABI エクスポート、およびドライバー実装をパッケージ化しています。

## コネクタ

- 拡張 ID: `irodori.mongodb`
- エンジン ID: `mongodb`
- ワイヤープロトコル: `mongodb`
- デフォルトポート: `27017`
- ネイティブ ABI: `irodori.connector.native.v1`
- ドライバー連携: `あり`
- マーケットプレイス公開範囲: `公開`
- パッケージバージョン: `0.1.3`

パッケージには `db/mongo.rs` からのデスクトップアダプターのソーススナップショットが含まれています。

コネクタのメタデータは `connector.config.json` と `irodori.extension.json` にあります。
Rust クレートは `src/lib.rs` からネイティブ ABI をエクスポートし、共有 JSON/バッファヘルパーに `irodori-connector-abi` を使用し、コネクタの動作は `src/driver.rs` に保持しています。

## 接続メタデータ

- エンドポイントモード: `hostPort`, `connectionString`
- トランスポートモード: `direct`, `sshTunnel`, `socks5Proxy`, `httpConnectProxy`, `proxyChain`
- TLS 対応: `あり`
- デフォルトで TLS 必須: `いいえ`
- カスタムドライバーオプション: `あり`

### エンドポイントフィールド

| フィールド | ラベル | 型 | 必須 |
| --- | --- | --- | --- |
| `host` | ホスト | `string` | はい |
| `port` | ポート | `number` | いいえ |
| `database` | データベースまたはバケット | `string` | いいえ |

## 認証

コネクタはこれらの認証モードを広告し、クライアントが適切な認証情報フィールドを表示できるようにします。
ドライバー固有またはプロバイダー固有の値は必要に応じて `options` 経由で渡すことも可能です。

| 認証方式 | ラベル | 種類 | 秘密情報の用途 |
| --- | --- | --- | --- |
| `none` | 認証なし | `none` | なし |
| `connectionString` | 接続文字列 / DSN | `connectionString` | なし |
| `scram` | SCRAM ユーザー/パスワード | `userPassword` | `password` |
| `mongodbX509` | X.509 クライアント証明書 | `certificate` | `privateKey`, `privateKeyPassphrase` |
| `awsIam` | AWS IAM | `iam` | `token` |
| `awsDefaultCredentialsChain` | AWS デフォルト認証情報チェーン | `iam` | なし |
| `awsProfile` | AWS 共有設定プロファイル | `iam` | なし |
| `awsSso` | AWS IAM Identity Center / SSO | `iam` | `token` |
| `webIdentity` | AWS ウェブアイデンティティ | `iam` | `token` |
| `awsAssumeRole` | AWS STS ロール引き受け | `iam` | `token` |
| `kerberos` | Kerberos / GSSAPI | `kerberos` | `token` |
| `ldap` | LDAP ユーザー/パスワード | `userPassword` | `password` |
| `oidc` | OIDC / ワークロードアイデンティティ | `oauth2` | `token` |
| `customDriverOptions` | カスタムドライバーオプション | `custom` | `password`, `token`, `privateKey`, `privateKeyPassphrase` |

## ネイティブ ABI 呼び出し

| メソッド | レスポンス |
| --- | --- |
| `health` | コネクタのヘルス、エンジン ID、ABI バージョン、ドライバー状態を返します。 |
| `describe` | 埋め込みマニフェストとコネクタ設定を返します。 |
| `manifest` | 生の `irodori.extension.json` を返します。 |
| `config` | 生の `connector.config.json` を返します。 |
| `connect` | ネイティブコネクタ接続を開き検証します。 |
| `query` | コネクタクエリを実行し、構造化された行または JSON 結果を返します。 |
| `metadata` | スキーマ、テーブル、カラム、インデックス、コレクション、または同等のメタデータを読み取ります。 |
| `close` | キャッシュされたネイティブ接続を閉じて削除します。 |

## 開発

このチェックアウト内のすべての拡張クレートは `../target` を共有しているため、依存関係は兄弟リポジトリ間で一度だけコンパイルされます。

```sh
make check
make build
```

リリースパッケージはプラットフォーム固有のネイティブアーティファクトを `dist/native` に配置します。

## ライセンス

0BSD。ほぼあらゆる目的でこのプロジェクトを使用、コピー、修正、配布できます。
# esctl

A full-featured Elasticsearch CLI written in Rust.

## Build

```bash
cargo build --release
# Binary at: ./target/release/esctl
```

## Configuration

Config file lives at `~/.esctl/config.toml`:

```toml
default_profile = "local"

[profiles.local]
host = "http://localhost:9200"
auth = "basic"
username = "elastic"

[profiles.prod]
host = "https://my-cluster.example.com:9243"
auth = "apikey"
```

Passwords and API keys are stored in the OS keyring (not in the config file).

## Authentication

```bash
# Basic auth via flags
esctl --user elastic --password secret cluster health

# API key via flag
esctl --api-key "abc123==" cluster health

# Via environment variables
ESCTL_HOST=http://localhost:9200 ESCTL_USER=elastic ESCTL_PASSWORD=secret esctl cluster health
```

## Output Formats

```bash
esctl --output json index list        # pretty colored JSON (default)
esctl --output table index list       # human-friendly table
esctl --output json --no-color ...    # plain JSON (CI-friendly)
```

## Profiles

```bash
esctl profile add local --host http://localhost:9200 --auth basic --user elastic
esctl profile add prod  --host https://prod.es.io    --auth apikey --api-key "abc=="
esctl profile list
esctl profile use prod
esctl profile show local
```

## Cluster

```bash
esctl cluster health
esctl cluster health --level indices
esctl cluster info
esctl cluster stats
esctl cluster nodes
esctl cluster nodes --verbose
```

## Indices

```bash
esctl index list
esctl index list --pattern "logs-*"
esctl index create my-index --shards 2 --replicas 1
esctl index create my-index --body ./settings.json
esctl index delete my-index
esctl index delete my-index --yes        # skip confirmation
esctl index open  my-index
esctl index close my-index
esctl index stats my-index
```

## Mappings & Settings

```bash
esctl mapping get my-index
esctl mapping put my-index --body ./mapping.json
esctl mapping put my-index --body -       # read from stdin

esctl settings get my-index
esctl settings get my-index --include-defaults
esctl settings put my-index --body ./settings.json
```

## Aliases

```bash
esctl alias list
esctl alias list --index my-index
esctl alias add  my-alias --index my-index
esctl alias remove my-alias --index my-index
```

## Documents

```bash
# Get
esctl doc get my-index abc123

# Index (create/replace)
esctl doc index my-index --body ./doc.json
esctl doc index my-index --id abc123 --body ./doc.json
echo '{"name":"foo"}' | esctl doc index my-index --body -

# Update
esctl doc update my-index abc123 --body ./partial.json

# Delete
esctl doc delete my-index abc123

# Bulk NDJSON
esctl doc bulk my-index ./records.ndjson
cat records.ndjson | esctl doc bulk my-index

# Write (interactive / piped)
echo '{"name":"bar"}' | esctl doc write my-index
esctl doc write my-index --id xyz --body ./doc.json
```

## Search

```bash
# Simple query string
esctl search run my-index --query "status:active AND age:>30"

# Full DSL
esctl search run my-index --dsl ./query.json
cat query.json | esctl search run my-index --dsl -

# With aggregations
esctl search run my-index --dsl ./query.json --agg ./aggs.json

# Pagination
esctl search run my-index --query "*" --size 50 --from 100

# Sort
esctl search run my-index --query "*" --sort "timestamp:desc"

# Scroll through all results
esctl search scroll my-index --dsl ./query.json --size 500
```

## Snapshots

```bash
# Repositories
esctl snapshot repo list
esctl snapshot repo create my-repo --type fs --settings ./repo-settings.json
esctl snapshot repo delete my-repo

# Snapshots
esctl snapshot list my-repo
esctl snapshot create my-repo snap-2024 --indices "logs-*" --wait
esctl snapshot restore my-repo snap-2024 --indices "logs-*" --wait
esctl snapshot delete my-repo snap-2024
esctl snapshot status
esctl snapshot status my-repo snap-2024
```

## Reindex

```bash
esctl reindex --src old-index --dst new-index
esctl reindex --src old-index --dst new-index --wait
esctl reindex --src old-index --dst new-index --query ./filter.json
```

## Security

```bash
# Users
esctl user list
esctl user get john
esctl user create john --roles viewer,editor --password secret123
esctl user delete john

# Roles
esctl role list
esctl role get viewer
esctl role create my-role --body ./role.json
esctl role delete my-role

# API Keys
esctl apikey create --name ci-key --expiry 30d
esctl apikey list
esctl apikey invalidate abc123
```

## Stdin Chaining Examples

```bash
# Pipe a document directly
echo '{"event":"login","user":"alice"}' | esctl doc write events

# Pipe a query and format as table
cat query.json | esctl search run my-index --dsl - --output table

# Bulk load from compressed file
gunzip -c dump.ndjson.gz | esctl doc bulk my-index

# Chain: search and reindex filtered results
cat filter.json | esctl reindex --src raw --dst clean --query -
```

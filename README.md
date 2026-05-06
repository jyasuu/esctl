# esctl

A full-featured Elasticsearch CLI written in Rust.

## Global Flags

These flags work with every subcommand:

```bash
esctl [FLAGS] <subcommand>

  -p, --profile <name>       Connection profile  (env: ESCTL_PROFILE)
      --host <url>           Override ES host     (env: ESCTL_HOST)
  -u, --user <username>      Basic auth user      (env: ESCTL_USER)
      --password <pass>      Basic auth password  (env: ESCTL_PASSWORD)
      --api-key <key>        API key auth         (env: ESCTL_API_KEY)
  -o, --output json|table    Output format        (default: json)
      --no-color             Disable ANSI colors  (env: ESCTL_INSECURE)
      --timeout <secs>       Request timeout      (env: ESCTL_TIMEOUT, default: 30)
      --insecure             Skip TLS verification
```



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
# Health — simple
esctl cluster health
esctl cluster health --level indices
esctl cluster health --level shards
esctl cluster health --output table          # colored status board

# Health — wait for green (useful in CI/CD)
esctl cluster health --wait-for-status green --timeout 60s

# Info (version, node name, UUID)
esctl cluster info
esctl cluster info --output table            # clean summary view

# Stats
esctl cluster stats
esctl cluster stats --summary                # condensed key metrics
esctl cluster stats --summary --output table # human-readable table

# Nodes
esctl cluster nodes
esctl cluster nodes --output table           # compact table with heap/cpu coloring
esctl cluster nodes --verbose                # full JVM/OS detail
esctl cluster nodes --role master            # filter by role
esctl cluster nodes --role data

# Pending tasks
esctl cluster pending

# Cluster settings
esctl cluster settings
esctl cluster settings --include-defaults

# Update a cluster setting
esctl cluster set cluster.routing.allocation.enable=all
esctl cluster set cluster.routing.allocation.enable=none --type persistent
esctl cluster set indices.recovery.max_bytes_per_sec=100mb

# Allocation explain (why is a shard unassigned?)
esctl cluster allocation-explain
esctl cluster allocation-explain --index my-index --shard 0 --primary true
esctl cluster allocation-explain --output table   # formatted decision board

# Reroute
esctl cluster reroute --retry-failed
esctl cluster reroute --retry-failed --dry-run    # simulate only
```

## Indices

```bash
# List
esctl index list
esctl index list --pattern "logs-*"
esctl index list --output table                   # colored health/status table
esctl index list --sort docs.count                # sort by doc count
esctl index list --health open                    # open indices only
esctl index list --health closed                  # closed indices only
esctl index list --system                         # include . system indices

# Create
esctl index create my-index
esctl index create my-index --shards 3 --replicas 1
esctl index create my-index --body ./settings-and-mappings.json
cat settings.json | esctl index create my-index --body -

# Delete (single or multi)
esctl index delete my-index
esctl index delete my-index --yes                 # skip confirmation
esctl index delete idx-a idx-b idx-c --yes        # delete multiple

# Open / Close
esctl index open   my-index
esctl index open   my-index --wait-for-active-shards all
esctl index close  my-index
esctl index close  my-index --yes

# Stats
esctl index stats my-index
esctl index stats my-index --summary              # key metrics only
esctl index stats my-index --summary --output table

# Info (settings + mapping combined)
esctl index info my-index
esctl index info my-index --output table          # settings + field list

# Maintenance
esctl index refresh    my-index
esctl index flush      my-index
esctl index forcemerge my-index
esctl index forcemerge my-index --max-num-segments 1 --wait
esctl index clear-cache my-index
esctl index clear-cache my-index --cache field,query

# Clone & Shrink
esctl index clone my-index my-index-clone
esctl index clone my-index my-index-clone --shards 1 --replicas 0
esctl index shrink big-index small-index --shards 1

# Recovery & Segments
esctl index recovery my-index
esctl index segments  my-index
```

## Mappings

```bash
esctl mapping get my-index                        # full mapping JSON
esctl mapping get my-index --flat                 # flat field:type list
esctl mapping get my-index --flat --output table  # as a table
esctl mapping fields my-index                     # alias for --flat
esctl mapping put my-index --body ./mapping.json
esctl mapping put my-index --body -               # from stdin
```

## Settings

```bash
esctl settings get my-index
esctl settings get my-index --include-defaults
esctl settings get my-index --key number_of_replicas
esctl settings get my-index --output table        # key/value table
esctl settings put my-index --body ./settings.json
esctl settings set my-index number_of_replicas=2
esctl settings set my-index routing.allocation.enable=none
```

## Aliases

```bash
# List
esctl alias list
esctl alias list --index my-index
esctl alias list --name  my-alias
esctl alias list --output table

# Add
esctl alias add my-alias --index my-index
esctl alias add my-alias --index my-index --is-write-index
esctl alias add my-alias --index my-index --routing shard-1
esctl alias add my-alias --index my-index --filter ./query.json

# Remove
esctl alias remove my-alias --index my-index

# Move atomically (remove + add in one request)
esctl alias move my-alias --from old-index --to new-index
esctl alias move my-alias --from old-index --to new-index --is-write-index

# Raw actions body
esctl alias actions ./actions.json
cat actions.json | esctl alias actions -
```

## Documents

```bash
# Get
esctl doc get my-index abc123
esctl doc get my-index abc123 --source-only          # just _source
esctl doc get my-index abc123 --fields name,status   # specific fields
esctl doc get my-index abc123 --output table         # metadata + field table

# Exists (exit 0 = found, exit 1 = not found)
esctl doc exists my-index abc123

# Index (create or replace)
esctl doc index my-index --body ./doc.json
esctl doc index my-index --id abc123 --body ./doc.json
esctl doc index my-index --body - --pipeline my-pipeline
echo '{"name":"alice"}' | esctl doc index my-index --body -

# Create (fails with 409 if ID already exists)
esctl doc create my-index abc123 --body ./doc.json

# Update (partial doc)
esctl doc update my-index abc123 --body ./partial.json
esctl doc update my-index abc123 --body - --retry-on-conflict 5
# Update via inline Painless script
esctl doc update my-index abc123 --script "ctx._source.count += 1"
# Upsert
esctl doc update my-index abc123 --body ./partial.json --upsert ./default.json

# Delete
esctl doc delete my-index abc123
esctl doc delete my-index abc123 --if-seq-no 5 --if-primary-term 1

# Multi-get
esctl doc mget my-index id-1 id-2 id-3
esctl doc mget my-index id-1 id-2 --source-only
esctl doc mget my-index id-1 id-2 --output table

# Count
esctl doc count my-index
esctl doc count my-index --query "status:active"
esctl doc count my-index --dsl ./query.json
esctl doc count my-index --output table          # prints "Count: 1,234"

# Delete by query
esctl doc delete-by-query my-index --query "status:deleted"
esctl doc delete-by-query my-index --dsl ./filter.json --wait
esctl doc delete-by-query my-index --dsl ./filter.json --slices 4

# Update by query (Painless script)
esctl doc update-by-query my-index --script "ctx._source.active = true"
esctl doc update-by-query my-index --dsl ./script-and-query.json --wait

# Bulk NDJSON
esctl doc bulk my-index ./records.ndjson
cat records.ndjson | esctl doc bulk my-index
esctl doc bulk my-index ./records.ndjson --batch 1000   # batched with progress bar
esctl doc bulk my-index ./records.ndjson --show-errors 20
esctl doc bulk my-index ./records.ndjson --pipeline my-pipeline

# Write (single doc, stdin-first)
echo '{"event":"login"}' | esctl doc write my-index
esctl doc write my-index --id abc123 --body ./doc.json
esctl doc write my-index --create                       # fail if exists

# Ingest (pipe JSON array or NDJSON, auto-batched)
cat events.json   | esctl doc ingest my-index
cat events.ndjson | esctl doc ingest my-index --batch 500
esctl doc ingest my-index --body ./data.json --id-field "user_id"
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

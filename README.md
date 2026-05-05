# esctl

---

## 🦀 `esctl` — Rust CLI for Elasticsearch

### Core Crates

| Crate | Purpose |
|---|---|
| `clap` (v4, derive) | Command parsing & subcommand hierarchy |
| `reqwest` (async + rustls) | HTTP client for ES REST API |
| `tokio` | Async runtime |
| `serde` / `serde_json` | JSON serialization |
| `comfy-table` | Table output formatting |
| `config` + `toml` | Config file (~/.esctl/config.toml) |
| `keyring` | Secure credential storage |
| `indicatif` | Progress bars (bulk ops, snapshots) |
| `colored` | Colored terminal output |
| `anyhow` / `thiserror` | Error handling |
| `atty` / `is-terminal` | Detect stdin piping |

---

### Project Structure

```
esctl/
├── Cargo.toml
├── src/
│   ├── main.rs                  # Entry point, CLI dispatch
│   ├── cli/
│   │   ├── mod.rs               # Root Cli struct (clap)
│   │   ├── index.rs             # index subcommands
│   │   ├── document.rs          # doc subcommands
│   │   ├── search.rs            # search subcommands
│   │   ├── cluster.rs           # cluster subcommands
│   │   ├── alias.rs             # alias subcommands
│   │   ├── mapping.rs           # mapping subcommands
│   │   ├── settings.rs          # settings subcommands
│   │   ├── snapshot.rs          # snapshot subcommands
│   │   ├── reindex.rs           # reindex subcommands
│   │   └── security.rs          # user/role/apikey subcommands
│   ├── client/
│   │   ├── mod.rs               # EsClient wrapper (reqwest)
│   │   └── auth.rs              # Auth resolver (basic / apikey)
│   ├── config/
│   │   ├── mod.rs               # Config load/save
│   │   └── profile.rs           # Named connection profiles
│   ├── output/
│   │   ├── mod.rs               # Output dispatcher
│   │   ├── json.rs              # Pretty JSON printer
│   │   └── table.rs             # Table renderer (comfy-table)
│   └── stdin.rs                 # Stdin reader / NDJSON stream
```

---

### Command Hierarchy

```
esctl [--profile <name>] [--host <url>] [--output json|table] [--no-color]

  # Connection profiles
  esctl profile list
  esctl profile add <name> --host <url> [--user <u>] [--api-key <k>]
  esctl profile use <name>

  # Cluster
  esctl cluster health [--level cluster|indices|shards]
  esctl cluster info
  esctl cluster stats
  esctl cluster nodes [--verbose]

  # Index
  esctl index list [--pattern <glob>]
  esctl index create <name> [--shards N] [--replicas N] [--body file.json|-]
  esctl index delete <name> [--yes]
  esctl index open <name>
  esctl index close <name>
  esctl index stats <name>

  # Mappings & Settings
  esctl mapping get <index>
  esctl mapping put <index> --body file.json|-
  esctl settings get <index> [--include-defaults]
  esctl settings put <index> --body file.json|-

  # Aliases
  esctl alias list [--index <name>]
  esctl alias add <alias> --index <name>
  esctl alias remove <alias> --index <name>

  # Documents
  esctl doc get <index> <id>
  esctl doc index <index> [--id <id>] --body file.json|-
  esctl doc update <index> <id> --body file.json|-
  esctl doc delete <index> <id>
  esctl doc bulk <index> [file.ndjson|-]       # stdin NDJSON support
  esctl doc write <index> [--id <id>]          # interactive / stdin pipe

  # Search
  esctl search <index> [--query <str>] [--dsl file.json|-]
                       [--size N] [--from N] [--sort field:asc]
                       [--agg file.json|-]
  esctl search scroll <index> --dsl file.json  # paginate all results

  # Snapshots
  esctl snapshot repo list
  esctl snapshot repo create <name> --type fs --settings file.json|-
  esctl snapshot list <repo>
  esctl snapshot create <repo> <name> [--indices <i1,i2>] [--wait]
  esctl snapshot restore <repo> <name> [--indices <i1,i2>] [--wait]
  esctl snapshot delete <repo> <name>
  esctl snapshot status [<repo> <name>]

  # Reindex
  esctl reindex --src <index> --dst <index> [--query file.json|-] [--wait]

  # Security
  esctl user list
  esctl user get <username>
  esctl user create <username> --roles role1,role2 --password <p>
  esctl user delete <username>
  esctl role list
  esctl role get <name>
  esctl role create <name> --body file.json|-
  esctl role delete <name>
  esctl apikey create --name <n> [--expiry 30d]
  esctl apikey list
  esctl apikey invalidate <id>
```

---

### Stdin Strategy

Three modes detected automatically:

```
# 1. File path
esctl doc index my-index --body ./doc.json

# 2. Explicit stdin flag
esctl search my-index --dsl -

# 3. Piped stdin (auto-detected via is-terminal)
cat query.json | esctl search my-index
echo '{"name":"foo"}' | esctl doc write my-index --id 1

# 4. NDJSON bulk streaming
cat records.ndjson | esctl doc bulk my-index
```

`stdin.rs` will handle: detecting TTY, reading to string (for single JSON), or streaming line-by-line (for NDJSON bulk with progress bar).

---

### Config File (`~/.esctl/config.toml`)

```toml
default_profile = "local"
default_output = "json"   # or "table"

[profiles.local]
host = "http://localhost:9200"
auth = "basic"
username = "elastic"
# password stored in OS keyring

[profiles.prod]
host = "https://my-cluster.example.com:9243"
auth = "apikey"
# api_key stored in OS keyring
```

Global flags override config: `--host`, `--user`, `--api-key`, `--output`, `--profile`.

---

### Output Layer

Every command returns a `serde_json::Value`. The output module routes it:

- **`--output json`** → `colored_json` pretty print
- **`--output table`** → `comfy-table` with auto-detected columns from top-level keys
- **`--no-color`** → strips ANSI (useful for scripting / CI)
- Exit codes: `0` success, `1` ES error (prints `error.reason`), `2` CLI/config error

---

### Implementation Order

1. **Scaffold** — Cargo workspace, clap CLI skeleton, config loader, EsClient wrapper
2. **Cluster** — health, info, nodes (validates client works end-to-end)
3. **Index** — full CRUD + stats
4. **Mappings + Settings + Aliases**
5. **Documents** — get/index/update/delete + bulk + stdin pipe
6. **Search** — query string, DSL body, scroll
7. **Snapshots + Reindex**
8. **Security**
9. **Polish** — table output, colors, progress bars, error messages

---

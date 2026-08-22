# research_hub_mcp

Locally maintained Rust policy and integration layer for the research workflow.
It wraps the pinned [`paper-search-mcp`](../paper-search-mcp) checkout instead
of duplicating every provider, while retaining the existing Rust tools as a
temporary compatibility fallback.

## Architecture

```text
MCP client
   |
research_hub_mcp (policy, stable tool names, DB integration)
   |-- discovery/download --> paper-search-mcp --> scholarly/OA providers
   `-- explicit save/export --> bib-manager API --> PostgreSQL
```

The invariants are deliberate:

- Search is discovery-only and never mutates the library.
- Only `save_papers_to_library` creates or updates canonical entries.
- `generate_bibliography` reads selected keys from the database; it does not
  synthesize a parallel in-memory bibliography.
- `.bib` files are derived interchange/build artifacts.
- `download_paper` delegates to upstream's open-access fallback. Sci-Hub is
  disabled by default.
- Zotero is not part of this stack.

The Python upstream currently exposes 21 source adapters. Keeping it as a
pinned submodule gives broad discovery coverage while this wrapper owns the
stable interface, selection policy, and database boundary.

## Toolchain and build

- Rust toolchain: `1.98.0` (pinned in `rust-toolchain.toml`)
- MSRV: `1.88`
- MCP Rust SDK (`rmcp`): `3.1.4`
- `paper-search-mcp`: pinned submodule revision based on `0.1.4`

```bash
git submodule update --init --recursive
cd ../paper-search-mcp && ~/.local/bin/uv sync --all-extras
cd ../research_hub_mcp
cargo build --release --locked
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

Using the Linux `~/.local/bin/uv` path matters under WSL: a Windows `uv.exe`
found through a mounted Windows PATH may fail with `Permission denied`. The
default configuration prefers the native Linux executable automatically.

## Configuration

Defaults locate the sibling `../paper-search-mcp` checkout and use
`http://127.0.0.1:8000` for the bibliography manager API. They can be
overridden without editing a config file:

```bash
export RSH_PAPER_SEARCH_COMMAND="$HOME/.local/bin/uv"
export RSH_PAPER_SEARCH_PROJECT_DIR="/path/to/paper-search-mcp"
export RSH_LIBRARY_API_URL="https://library.example.com"
```

The library service holds `DATABASE_URL`; this MCP process never needs direct
database credentials.

## MCP tools

- `search_papers`: delegates query, source selection, year filter, and result
  limits to `paper-search-mcp`.
- `download_paper`: delegates open-access resolution and download fallback.
- `save_papers_to_library`: explicitly persists selected normalized entries.
- `generate_bibliography`: requests a full or key-selected DB-derived BibTeX
  export.
- `extract_metadata`, `search_code`, `categorize_papers`: retained local Rust
  capabilities.

## Live tests

Unit and contract tests do not require third-party scholarly services. Provider
E2E tests are opt-in because public APIs can throttle or become temporarily
unavailable:

```bash
RUN_LIVE_TESTS=true cargo test --test providers_e2e_test --locked
cargo test --test paper_search_wrapper_test --locked -- --ignored
```

The wrapper test launches the actual pinned Python MCP child and validates a
Crossref search end to end.

## License

GPL-3.0, following the original `rust-research-mcp` codebase.

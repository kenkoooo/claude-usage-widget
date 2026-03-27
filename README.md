# claude-usage-widget

A GNOME Shell extension that shows your **Claude Code usage limits** in the top panel — at a glance, no browser tab required.

```
🌸 5%/6% 4d23h
```

The three numbers mean:
- **5%** — 5-hour window utilization
- **6%** — 7-day window utilization
- **4d23h** — time until the weekly limit resets

Usage data is fetched directly from Anthropic's API using the OAuth token that Claude Code already stores locally. Responses are cached for 5 minutes to stay well within rate limits.

## Requirements

- Ubuntu with GNOME Shell 45+
- [Claude Code](https://claude.ai/code) installed and signed in (provides the OAuth token)
- Rust toolchain (`curl` for HTTP, already on Ubuntu)
- `zip`, `gnome-extensions` CLI

## Installation

**1. Build and package**

```bash
./pack.sh
```

This produces two files in the project root:
- `claude-usage` — the helper binary
- `claude-usage@kenkoooo.zip` — the GNOME extension bundle

**2. Install the binary**

```bash
mkdir -p ~/.local/bin
cp claude-usage ~/.local/bin/
```

Make sure `~/.local/bin` is on your `PATH`. If not, add this to `~/.bashrc` or `~/.zshrc`:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

**3. Install the GNOME extension**

```bash
gnome-extensions install --force claude-usage@kenkoooo.zip
gnome-extensions enable claude-usage@kenkoooo
```

**4. Restart GNOME Shell**

On Wayland (Ubuntu default): **log out and log back in**.

The widget will appear in your top panel immediately after login.

## How it works

```
GNOME panel
  └─ extension.js          polls every 60s
       └─ claude-usage --panel
            └─ ~/.cache/claude-usage-widget/usage.json   (5-min cache)
                 └─ GET https://api.anthropic.com/api/oauth/usage
                      └─ ~/.claude/.credentials.json     (OAuth token)
```

The binary reads your existing Claude Code OAuth token — no separate API key needed.

## Updating

After changing any source file, re-run `./pack.sh` and repeat steps 2–4.

## Project structure

```
claude-usage-widget/
├── src/main.rs          Rust binary — fetches & formats usage data
├── extension/
│   ├── metadata.json    GNOME extension metadata
│   ├── extension.js     Panel widget (calls the binary every 60s)
│   └── icons/
│       └── claude-symbolic.svg   Panel icon
├── Cargo.toml
└── pack.sh              Build + package script
```

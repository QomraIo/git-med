---
title: git-med remote
description: Manage tracked remote repository endpoints and URLs in configuration
---

## How to use
- `git-med remote add <name> <url>`
- `git-med remote create <repo_url_or_name> [--server <url>] [--visibility <public|protected|private>] [--description <text>]`

## What
Manages tracked remote repository endpoints stored in `.git-med/config`, and creates/initializes remote repositories on a `git-med-forge` server (`POST /create_repo`) before pushing.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Configuration Storage | INI format in `.git-med/config` | INI format in `.git/config` | Same INI section layout (`[remote "<name>"] url = ...`). |
| Supported Subcommands | `add`, `create`, `-v` (list) | `add`, `remove`/`rm`, `set-url`, `get-url`, `show`, `-v`, `rename`, `prune` | `git-med remote create` provisions the repository directly on `git-med-forge` using saved credentials. |
| URL Validation | Validates remote name and format constraints | Enforces Git remote naming rules | Both reject invalid characters (whitespace, slashes, special symbols). |

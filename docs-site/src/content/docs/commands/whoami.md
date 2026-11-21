---
title: git-med whoami
description: Display currently authenticated username and server origin
---

## How to use
`git-med whoami --server <server_url>`

## What
Displays the currently authenticated username and server origin retrieved from the local credential store.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Purpose | Checks current authenticated identity against a server | `gh auth status` or `git config user.name` | Displays server-specific active authentication state. |
| Targeting | Accepts `--server <url>` to query credentials for a specific forge instance | Checks global configuration or active helper session | Queries the origin-keyed credential store for the specified server. |
| Output Information | Outputs `Logged in as: <username> (<server_origin>)` or `Not logged in to <server_origin>` | Displays account name, active token scopes, and protocols | Concise single-line identity report for the target forge instance. |

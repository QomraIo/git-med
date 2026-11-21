---
title: git-med logout
description: Remove stored credentials and access tokens for a specific server origin
---

## How to use
`git-med logout --server <server_url>`

## What
Removes stored authentication credentials and access tokens for the specified server origin from the local credential store.

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Purpose | Clears stored session tokens for a remote server | `gh auth logout` or `git credential reject` | Removes target server entries from the local `git-med` credentials store. |
| Targeting | Targets specific server origin via `--server <url>` (defaults to `http://localhost:3000`) | Erases cached credentials matching protocol/host tuple | Scoped directly to the target server origin. |
| State Verification | Informs user whether credentials were removed or if no entry existed | Silently removes or updates helper cache | Provides immediate terminal feedback on logout status. |

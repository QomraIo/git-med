# git-med remote

## How to use
`git-med remote add <name> <url>`

## What
Manages tracked remote repository endpoints and URLs stored in the repository configuration file (`.git-med/config`).

## Git similarity
| Aspect | git-med | Git | Key Differences |
| --- | --- | --- | --- |
| Configuration Storage | INI format in `.git-med/config` | INI format in `.git/config` | Same INI section layout (`[remote "<name>"] url = ...`). |
| Supported Subcommands | `add`, `remove`/`rm`, `set-url`, `get-url`, `show`, `-v` (list) | `add`, `remove`/`rm`, `set-url`, `get-url`, `show`, `-v`, `rename`, `prune` | Core remote management subcommands match standard Git syntax. |
| URL Validation | Validates remote name and format constraints | Enforces Git remote naming rules | Both reject invalid characters (whitespace, slashes, special symbols). |

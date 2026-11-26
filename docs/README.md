# git-med Documentation

Welcome to the documentation for `git-med`, a specialized distributed version control system designed for biomedical imaging datasets .

---

## Quick Navigation

* **[CLI Commands Reference](file:///root/git-med/docs/commands.md)**: Index and quick references for all 15 CLI commands.
* **[Authentication Guide](file:///root/git-med/docs/authentication.md)**: Detailed documentation on authentication mechanisms, token storage, Auth0 SSO, and access control.
* **[DICOM Tag Rules](file:///root/git-med/docs/dcm_tags_rules.md)**: Standard DICOM tags organized by their Information Entity (IE) level.

---

## All CLI Commands

| Command | Usage | Description |
| --- | --- | --- |
| [`init`](file:///root/git-med/docs/commands/init.md) | `git-med init <repository_name>` | Initialize a new repository |
| [`add`](file:///root/git-med/docs/commands/add.md) | `git-med add` | Stage changes and extract metadata |
| [`commit`](file:///root/git-med/docs/commands/commit.md) | `git-med commit -m "<message>"` | Commit staged tree to history |
| [`branch`](file:///root/git-med/docs/commands/branch.md) | `git-med branch <branch_name>` | Create and switch to a branch |
| [`remote`](file:///root/git-med/docs/commands/remote.md) | `git-med remote add <name> <url>` | Configure remote endpoints |
| [`view`](file:///root/git-med/docs/commands/view.md) | `git-med view <smart\|dicom\|brats>` | Query or transform dataset format |
| [`clone`](file:///root/git-med/docs/commands/clone.md) | `git-med clone <url> [destination]` | Clone remote repository and assets |
| [`push`](file:///root/git-med/docs/commands/push.md) | `git-med push [remote] [branch]` | Upload commits and LFS payloads |
| [`pull`](file:///root/git-med/docs/commands/pull.md) | `git-med pull [remote] [branch]` | Fetch changes and fast-forward |
| [`merge`](file:///root/git-med/docs/commands/merge.md) | `git-med merge <branch>... [--left\|--right]` | Multi-parent merge with resolution |
| [`mix`](file:///root/git-med/docs/commands/mix.md) | `git-med mix <result_branch> <specs>...` | Cross-repository cohort mixing |
| [`login`](file:///root/git-med/docs/commands/login.md) | `git-med login --server <url>` | Authenticate and cache credentials |
| [`logout`](file:///root/git-med/docs/commands/logout.md) | `git-med logout --server <url>` | Remove cached server credentials |
| [`whoami`](file:///root/git-med/docs/commands/whoami.md) | `git-med whoami --server <url>` | Display current authenticated user |
| [`generate`](file:///root/git-med/docs/commands/generate.md) | `git-med generate <type> <output_dir>` | Generate synthetic test data |

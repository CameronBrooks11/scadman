# Manifest and lockfile reference

A scadman project is a directory holding `scadman.toml`, which you write, and
`scadman.lock`, which scadman writes. Commit both. Run scadman from that directory; it
does not search parent directories.

## `scadman.toml`

```toml
[project]
name = "enclosure"
openscad = ">=2021.01"        # optional

[dependencies]
BOSL2 = { git = "https://github.com/BelfrySCAD/BOSL2", branch = "master" }
Round-Anything = { git = "https://github.com/Irev-Dev/Round-Anything", rev = "061fef7c429628808e847696bb345a9b0ec6e279" }
dotSCAD = { git = "https://github.com/JustinSDK/dotSCAD", rev = "bb33edfd75cba0edbb7971606a077b9c69d0b7d2", root = "src", on_path = true }
mylib = { path = "../mylib" }
```

### `[project]`

| Key | Required | Meaning |
|---|---|---|
| `name` | yes | The project's name, shown by `graph`. |
| `openscad` | no | Minimum OpenSCAD version, as `"2021.01"` or `">=2021.01"`. `doctor` and `run` warn when the installed version is older; nothing is blocked. |

### `[dependencies]`

Each key is the name the library is exposed under: `BOSL2` makes `include <BOSL2/…>`
resolve. Use the name the library's own files use for themselves, since many libraries
import their own files as `<Name/…>`. A name is a single path component: no `/` or `\`,
and not `.` or `..`.

A git dependency:

| Key | Required | Meaning |
|---|---|---|
| `git` | yes | The repository URL. |
| `rev` | one of these three | An exact commit, as the full 40-character hash; an abbreviated one is refused. Never moves. |
| `tag` | | A tag. Locked to the commit it points at; `scadman update` re-reads it. |
| `branch` | | A branch. Locked to its tip at lock time; `scadman update` advances it. |
| `root` | no | Subdirectory to expose instead of the repository root. See [library-roots.md](library-roots.md). |
| `on_path` | no | Also put the exposed directory on `OPENSCADPATH`. See [library-roots.md](library-roots.md). |
| `submodules` | no | Also fetch the repository's git submodules. See [Submodules](#submodules). |

Give exactly one of `rev`, `tag` and `branch`; an entry with none, or with more than
one, is refused at lock time.

A path dependency takes `path` (a directory, relative to the project or absolute) and
the same optional `root` and `on_path`. See [path-dependencies.md](path-dependencies.md).

An unknown table or key anywhere in your `scadman.toml` is an error, so a misspelled
`[dependencies]` header or key cannot be silently ignored. A bare version string
(`BOSL2 = "^2.0"`) is reserved for a future registry and is refused at lock time.

### Submodules

A git dependency's submodules are left out by default: their folders are empty, and
`lock`, `update` and `sync` warn about each one. Set `submodules = true` on the
dependency to fetch them:

```toml
SBC_Case_Builder = { git = "https://github.com/hominoids/SBC_Case_Builder", rev = "5979914bc23aa0d85da592be4c37b6ed97c20ddf", submodules = true }
```

- Every submodule is fetched at the commit the dependency records for it, and so are
  the submodules of those. The dependency's own `rev` therefore still fixes the whole
  tree, and the lockfile needs no extra commits.
- It is all or nothing. A submodule that cannot be fetched fails `lock`, with git's own
  message: a repository that is gone, or an `ssh` URL (`git@github.com:…`) on a
  machine with no key for it. A library's own `update = none` setting for a submodule
  does not exempt it.
- If any `scadman.toml` among your dependencies asks for a library's submodules, they
  are fetched for every library that uses it, since a library has one copy per project.
- The content hash covers the submodule files, so the same commit locks to a different
  `hash` with and without them.
- Removing `submodules = true` does not make the lockfile out of date. Run
  `scadman lock` to lock the library without them. (A project with a path dependency
  resolves again on every `sync`, and drops them then, at the same commit.)

A path dependency is read from its directory as it stands, so check its submodules out
there yourself.

### One version per library

OpenSCAD has one flat namespace, so each library appears once in a project. `lock`
stops with an error, and writes no lockfile, when:

- one source is requested at two different commits;
- two different sources are given the same name;
- one source is given two names.

There is no override syntax yet; the fix is to make the declarations agree.

Two URLs are the same source when they match after a trailing `/` and a `.git` suffix
are removed. Other spellings of one repository, such as different letter case or `ssh`
against `https`, count as different sources
([#98](https://github.com/CameronBrooks11/scadman/issues/98)), so write a library's URL
the same way everywhere.

### Dependencies of dependencies

If a git dependency ships its own `scadman.toml`, its dependencies are resolved too,
under the same one-version rule. It may not declare path dependencies. Unknown keys
in a dependency's manifest are ignored, apart from those inside its dependency tables,
so a library written for a newer scadman stays readable.

Almost no OpenSCAD library ships a manifest today. When a library imports another
without declaring it, `sync` names the missing library and you add it to your own
manifest.

## `scadman.lock`

Generated by `lock` and `update`; do not edit it. One entry per resolved library:

```toml
version = 1

[[package]]
name = "BOSL2"
source = "https://github.com/BelfrySCAD/BOSL2"
rev = "afe82db884ee4409aa76ecfcfbbf54d446964af1"
hash = "…"
```

| Field | Meaning |
|---|---|
| `name` | The name the library is exposed under. |
| `source` | The git URL, or `path:` followed by the absolute directory of a path dependency. |
| `rev` | The exact commit. For a path dependency, its content hash. |
| `hash` | SHA-256 of the library's files: the key of its copy in the store. |
| `dependencies` | Names of the libraries this one declares, if any. |
| `root`, `on_path` | Copied from the manifest when set. |
| `submodules` | `true` when the content includes the library's git submodules. |

`version` is the lockfile format. It is `2` when any library is locked with its
submodules and `1` otherwise, so a project that does not use submodules keeps a
lockfile that scadman 0.1.0-alpha.8 and earlier still read. Every command refuses a
lockfile whose version it does not know, and `lock` and `update` will not overwrite
one: upgrade scadman, or delete the file to write a fresh one.

On a machine that does not have a library's content yet, `sync` fetches `rev` from
`source` and refuses to continue if the content does not hash to `hash`.

`sync`, `run` and `env` use the lockfile as it is. They create it if it is missing, and
rewrite it when a path dependency's content has changed. They stop and ask for
`scadman lock` when the manifest and the lockfile disagree:

- the manifest declares a library the lockfile lacks;
- the lockfile holds a library the manifest no longer leads to, as after
  `scadman remove`;
- a library's source, `rev`, `root` or `on_path` no longer matches;
- the manifest asks for a library's submodules and the lockfile has it without them.

One edit is not detected: a changed `tag` or `branch` name on an unchanged URL. Running
`scadman lock` after every manifest edit covers it.

## Where scadman keeps things

| Path | What | Commit it? |
|---|---|---|
| `scadman.toml` | The manifest. | yes |
| `scadman.lock` | The lockfile. | yes |
| `.scadman/env/` | The project environment: one entry per library: a symlink, or on Windows a tree of hard links to the stored files (copies where a hard link can't be made), rebuilt on every `sync`. | no (`init` ignores it) |
| `$XDG_DATA_HOME/scadman/store/`, or else `~/.local/share/scadman/store/` (Linux, macOS) or `%LOCALAPPDATA%\scadman\store\` (Windows) | The store: one read-only directory per content hash, shared by all projects. | not in the project |

A library's stored copy is its working tree without `.git`. Symbolic links and special
files (FIFOs, sockets, devices) inside a library are not copied. File modes do not affect
the hash.

### Stored libraries are read-only

Every project on the machine that locks a library at the same content shares one stored
copy, and `.scadman/env/` leads straight to it. So scadman takes write permission off a
copy before it is used: its files everywhere, and on Linux and macOS its directories
too. Editing a library's file through `.scadman/env/` fails, and a tool run from a
library's folder cannot leave files there. Python, for one, then skips writing
`__pycache__` and carries on.

To change a library, depend on your own copy by path
([path-dependencies.md](path-dependencies.md)), or fork it and depend on the fork.

A copy that is still write-protected has had no chance to change, so `sync`, `run` and
`env` take it as it stands. One that is not is re-hashed first, and if it no longer
matches the lockfile it is fetched again and replaced, with a warning. That covers a
store filled by a scadman older than this protection, on its first sync.
`scadman sync --verify` re-hashes every copy regardless, and `scadman doctor` reports any
that no longer matches on its `content:` line.

File permissions do not bind root. Where scadman and the tools around it run as root, as
in many CI containers, a stored file can still be edited in place and will still look
protected, so use `scadman sync --verify` there.

### Deleting the store

Nothing removes store entries yet
([#96](https://github.com/CameronBrooks11/scadman/issues/96)). Deleting the store is
safe: the next `sync` restores what the lockfile needs. Its entries are read-only, so
give the permission back first:

```sh
chmod -R u+w ~/.local/share/scadman/store && rm -rf ~/.local/share/scadman/store
```

On Windows, `Remove-Item -Recurse -Force "$env:LOCALAPPDATA\scadman\store"`.

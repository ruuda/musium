# Changelog

Musium does not currently tag particular releases, there is only the rolling
`master` branch. This will change at some point when it will be feature-complete
enough to warrant more stable releases. In the meantime though, there are
notable changes that require manual intervention, so these are listed here.

TODO: I should make some tags retroactively, probably that's better than
referencing random commits here.

## Unreleased

Published on 2023-02-TODO, commit TODO.

**Breaking changes:**

 * The `data_path` configuration option was renamed to `db_path`, Musium was not
   storing anything in the data path aside from the database anyway. Unlike
   `data_path`, `db_path` should include the file name.
 * Cover art thumbnails are now stored in the database. The `covers_path`
   configuration option has been removed. There is a script
   `tools/migrate_thumbnails.py` to import existing thumbnails into the
   database, so they do not have to be re-generated.

## 0.0 (7348a381)

Merged on 2023-02-24, commit `7348a381376d98bf77e1688e2ca82638e1692398`.

Internal changes:

 * The Nix-based development environment that was using a Nix 2.3-compatible
   `default.nix` has been replaced with a Flake that requires Nix 2.10 or later.

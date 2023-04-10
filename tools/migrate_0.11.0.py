#!/usr/bin/env python3

# Musium -- Music playback daemon with web-based library browser
# Copyright 2023 Ruud van Asseldonk
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# A copy of the License has been included in the root of the repository.

"""
migrate_0.11.0.py -- Migrate database from 0.10.1 to 0.11.0.

To migrate the database, back up the old database to a safe location, and ensure
that no file exists at the configured location. Then run "musium scan" with the
new version. This will create the database and populate it with files. Once it
starts loudness analysis, you can kill the process -- we can start the migration
then. Afterwards, run "musium scan" again to complete loudness analysis.

USAGE

  tools/migrate_0.11.0.py <old.sqlite3> <new.sqlite3>

  <old.sqlite3>   Path to the v0.10.1 Musium sqlite3 database.
  <new.sqlite3>   Path to the v0.11.1 Musium sqlite3 database.
"""

import sqlite3
import sys


def main(old_path: str, new_path: str) -> None:
    with sqlite3.connect(old_path) as conn_old:
        with sqlite3.connect(new_path) as conn_new:
            co = conn_old.cursor()
            cn = conn_new.cursor()

            # Migrate the import timestamps.
            co.execute("select imported_at, filename, mtime from file_metadata;")
            data = co.fetchall()
            cn.executemany(
                """
                update
                  files
                set
                  imported_at = ?
                where
                  filename = ? and mtime = ?;
                """,
                data,
            )
            conn_new.commit()


if __name__ == "__main__":
    if len(sys.argv) == 3:
        main(sys.argv[1], sys.argv[2])

    else:
        print(__doc__)
        sys.exit(1)

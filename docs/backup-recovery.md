# Backup and recovery contract

Laika treats a successful local save and a verified remote backup as separate
events. An acknowledged edit is saved synchronously in the SQLite catalog and
then converged to its XMP sidecar. The photo's backup state changes to backed up
only after every queued original/sidecar upload has passed the destination's
verification step.

## What is protected

- Originals are uploaded under immutable, content-versioned keys. Repeating an
  upload of the same bytes is idempotent; two different files with the same date
  and name cannot overwrite one another.
- Existing XMP sidecars are queued independently. Each changed byte sequence gets
  a new immutable key, so an older remote revision is not deleted by an edit.
- Local deletion and catalog removal issue no remote delete request.
- The durable SQLite queue survives application and network interruption. Pause
  stops new claims while already-running transfers finish and verify.
- The SQLite catalog is protected by local timestamped backups and by **Catalog →
  Export catalog + sidecars**. The portable bundle contains an integrity-checked
  `catalog.db`, byte-identical sidecars, and `manifest.json` with original paths,
  import-time BLAKE3 hashes, remote keys, and sidecar checksums.

Remote original/sidecar backup and catalog-bundle export are currently separate
operations. A green remote-backup state does not yet mean a catalog bundle was
uploaded to that destination.

## Recovery workflow

1. Preserve the damaged/current catalog before replacing anything.
2. Use **Catalog → Restore…** with a timestamped `catalog.db`, or copy
   `catalog.db` out of a portable bundle and restore that file.
3. Recreate the original folder tree from the remote objects named by the
   catalog's `remote_key` values and verify each file against the manifest/catalog
   BLAKE3 value before relinking it.
4. Place bundled XMP files beside their originals using the manifest mapping.
   Unknown namespaces, Adobe masks/profiles, and other foreign properties remain
   byte-identical in the bundle.
5. Open the restored catalog, run **Recheck missing**, relink moved folders when
   necessary, then run **Check integrity** and render/export representative edited
   photos.

An automated remote downloader and a recorded real-destination restore drill are
still required before U24 can be considered complete.

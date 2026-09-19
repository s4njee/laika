# Laika catalog and edit format

This directory is the public compatibility contract for Laika catalogs and
edits. It is intended to be sufficient for an independent reader or renderer;
the Rust source is not part of the specification.

- [Catalog](catalog.md) defines SQLite versioning, every schema revision, the
  tables that carry durable user data, and the JSON stored in edit rows.
- [Develop format](develop.md) defines all parameter slots, geometry, panel
  switches, and the pixel pipeline order.
- [XMP mapping](xmp.md) defines the sidecar mapping and Laika extensions.
- [Reference renderer](renderer.md) documents `laika-render` and its output
  contract.

## Stability promise

Laika's durable formats are additive. New versions may add tables, columns,
JSON object members, XMP attributes, or parameter slots. They will not change
the meaning of an existing field or reuse an existing parameter index. Readers
must ignore unknown fields. Writers should retain unknown XMP attributes and
namespace declarations.

Every released catalog schema remains readable by later Laika releases and by
the bundled `laika-render`. A catalog whose `schema_version` is newer than a
reader must be refused rather than guessed at. Version 0 means a catalog made
before explicit version stamping; it has the version-1 core tables and is read
using the same legacy fallbacks.

Numeric edit values are finite IEEE-754 numbers serialized as JSON numbers.
Malformed rows may be skipped, but must not change other photos. Paths are
UTF-8 text as accepted by SQLite; IDs are signed 64-bit SQLite integers.

## Compatibility rules for third-party readers

1. Open the database read-only and inspect `schema_version` if present.
2. Select the photo from `photos`, then its optional `edits` row.
3. Decode `params_json` as either the current object or the legacy bare array.
4. Pad a recognized legacy parameter width with the defaults in
   [Develop format](develop.md). Refuse an unknown width.
5. Resolve geometry and panel switches from the history entry immediately
   before `cursor` when one exists; otherwise use the values in `params_json`.
6. Apply panel bypasses, geometry, and stages in the documented order.
7. Treat absent sidecar values as unspecified, not as a reset.

The reference implementation of steps 3–6 is deliberately small and lives in
`laika_core::reference`; `laika-render` uses it without opening the GUI or
migrating the database.

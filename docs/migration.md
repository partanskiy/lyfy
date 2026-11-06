# Moving existing state into lyfy

This guide applies to any supported Arch Linux host. The
[example profile](../examples/migration/laptop.toml) uses synthetic account,
host, application and project names. It is not a deployment profile.
Keep real profiles, path inventories, migration plans and credentials outside
the project's source repository. Explicitly review publication before sharing
any captured state or configuration, including data already public elsewhere.

## Capture each host independently

Start with the existing deployed files. Export concrete results from the old
management system; do not replay its hooks or evaluate templates inside lyfy.
Inventory its files, packages, exclusions and generated/application data locally.
Classify unsupported state with an explicit reason and recovery prerequisite.

Adapt the synthetic profile's home, storage paths and curated includes. Review
each selected file for embedded credentials before using readable Git storage.
Use encrypted named backends for sensitive supported exports, and keep key
bootstrap outside snapshots. Begin with a small selection:

```sh
lyfy --config_file /absolute/private/config.toml save --selected_mounts config --dry_run
lyfy --config_file /absolute/private/config.toml save --selected_mounts config
lyfy --config_file /absolute/private/config.toml apply --selected_mounts config
```

Review the delta and coverage before publication. Repeat save without edits and
verify a reused snapshot with no file delta. Restore selected paths into a
disposable target and verify metadata and unaffected siblings. Overwrite and
prune are explicit permissions, not a way to dismiss unresolved conflicts.

Capture every additional host under its own identifier before sharing intent.
Review a common subset, publish it as a separate host and configure base_host
only for collections intended to share that state. Keep machine-specific
hardware, storage and boot configuration independent. Test a common update in
both directions while preserving each host's concrete exceptions.

## Verify recovery before replacing existing maintenance

- Match the local inventory against selected paths and explicit exclusions.
  Prefix policies are not globs. Declare nested Git and external mount boundaries.
- Retain exact package archives or pinned recipes and source inputs as needed.
  Verify acquisition, signature trust and pacman-managed build dependencies.
- Bind actual remote destinations outside published examples. Verify replicas,
  decryption bootstrap and an offline encrypted archive restore.
- Test identity/ACL mapping, persistent service state and privilege handoff in
  disposable targets before applying system resources to a real machine.
- Verify application consistency, storage attachment, firmware, disk unlock and
  boot/rescue prerequisites separately. Captured files do not prove bootability.

See [save/apply](save.md), [system recovery](system-recovery.md) and
[storage guarantees](storage.md). Provider fixtures and live-provider validation
are distinct evidence; record the scope of each recovery drill locally.

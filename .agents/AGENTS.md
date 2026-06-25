## Agent skills

### Issue tracker

Issues and PRDs live as local markdown files under `.agents/scratch/<feature>/`. See `.agents/docs/issue-tracker.md`.

### Triage labels

Five canonical triage roles, using the default label strings. See `.agents/docs/triage-labels.md`.

### Domain docs

Single-context: `.agents/CONTEXT.md` + `.agents/docs/adr/`. See `.agents/docs/domain.md`.

## Working discipline

- **Treat Bevy/Avian API knowledge as deprecated.** Both move fast; this project pins **Bevy 0.19** and **avian3d 0.7** (`Cargo.toml`). Verify every engine API against versioned docs or source *before* writing it — `docs.rs/bevy/0.19.0/…` / `docs.rs/avian3d/0.7.0/…`, or the git tags `v0.19.0` (bevyengine/bevy) and `v0.7.0` (Jondolf/avian) for example/source. This has repeatedly caught real renames (`Trigger`→`On`, buffered events→observers, `Camera` moving to `bevy::camera`, `SceneRoot`→`WorldAssetRoot`). Do not write engine code from memory.

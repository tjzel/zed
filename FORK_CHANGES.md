# Fork changes

This fork tracks Zed's **stable** release branch (`origin/v1.<N>.x`) and adds the changes below.
Everything here is additive to upstream Zed; nothing upstream is removed.

Keep this file up to date when adding, changing, or dropping a fork change, and re-check it after
each rebase onto a newer stable branch. When upstream ships an equivalent feature, drop the fork's
version instead of carrying both.

## Contents

1. [Per-token font size](#per-token-font-size)
2. [Theme syntax override fixes and additions](#theme-syntax-override-fixes-and-additions)
3. [Semantic token syntax conditions](#semantic-token-syntax-conditions)
4. [Compare against any commit](#compare-against-any-commit)
5. [Compare panel](#compare-panel)
6. [Single-file diffs against a base ref](#single-file-diffs-against-a-base-ref)
7. [Git panel: folder-scoped menu actions](#git-panel-folder-scoped-menu-actions)
8. [Notify instead of auto-update](#notify-instead-of-auto-update)
9. [Local build script fixes](#local-build-script-fixes)
10. [New and changed settings](#new-and-changed-settings)
11. [New actions](#new-actions)
12. [Dropped, because upstream now provides it](#dropped-because-upstream-now-provides-it)

## Per-token font size

Text runs can be shaped at a size that differs from the line's base size, so a single line can mix
sizes. Upstream shapes an entire line at one size — the size is a scalar threaded from `shape_line`
down through the platform shapers and the layout cache key.

- `gpui::HighlightStyle::font_size: Option<f32>` — a multiplier of the surrounding text size, not an
  absolute size, so it is independent of `buffer_font_size`. Multipliers compose multiplicatively
  when highlights layer.
- `gpui::TextStyle::font_size_scale` and `TextRun::font_size_scale` carry the multiplier into
  shaping; `FontRun::font_size` and `ShapedRun::font_size` carry it through layout and rasterization,
  and participate in the line layout cache key.
- Reachable from both highlight sources: `semantic_token_rules[].font_size` and
  `experimental.theme_overrides.syntax.<name>.font_size`.

Platform support: macOS (CoreText) and Windows (DirectWrite) shape per-run sizes. The `gpui_wgpu`
cosmic-text backend still shapes a line at one size; runs report the line size there, marked with a
`TODO` at the construction site.

## Theme syntax override fixes and additions

`crates/syntax_theme/src/syntax_theme.rs`, `crates/settings_content/src/theme.rs`

- **Fix: overrides refine instead of replace.** Overriding a capture name the theme does not define
  exactly (e.g. `keyword.control` when the theme defines `keyword`) inserted a bare entry containing
  only the overridden properties. Because that exact name then won prefix resolution, the theme's
  color was lost. New names now inherit the style the name currently resolves to, then overlay the
  properties you set. Covered by tests in that file.
- **Fix: `font_size` was dropped** when merging an override onto an existing entry.
- **New: `font_size`** in `HighlightStyleContent` (see [Per-token font size](#per-token-font-size)).
- **New: `underline`** in `HighlightStyleContent` — `true` underlines in the text color, or pass a
  color string. Upstream's `HighlightStyle` supports underline, but no settings field reaches it.

Both fields work in `experimental.theme_overrides` / `theme_overrides` and in theme files.

Known limitation: merging is refine-only, so `false`/absent means "no override" and a property a
theme sets cannot be un-set.

## Semantic token syntax conditions

`crates/settings_content/src/project.rs`, `crates/editor/src/semantic_tokens.rs`

Semantic token rules accept two matchers that consult tree-sitter at the token's position:

- `"syntax": [names]` — apply only if one of the capture names is present.
- `"not_syntax": [names]` — skip if any is present.

Names match exactly or as a dot-separated prefix (`variable` matches `variable.parameter`). Read
capture names for a position with `dev: open highlights tree view`.

This exists because some servers erase information in the token type. TypeScript (and so `tsgo`)
reclassifies function-typed parameters as `function` with a `declaration` modifier, making them
indistinguishable from real function declarations; tree-sitter still reports `variable.parameter`, so
a rule can exclude them:

```json
{
  "token_type": "function",
  "token_modifiers": ["declaration"],
  "not_syntax": ["variable.parameter"],
  "font_weight": "bold"
}
```

The tree-sitter capture query only runs for token types that have at least one conditioned rule.

## Compare against any commit

`git: compare with commit` opens a picker of the last 500 commits and diffs the working tree against
the selected one, reusing upstream's `BranchDiff` tab and its per-base-ref deduplication.

Upstream can only compare against a *branch* (`git: compare with branch`), and its branch picker
deliberately refuses free-text input, so there is no path to a sha. Supporting API:
`GitRepository::log_commits` and `Repository::log_commits`, returning `CommitSummary` values (local
repositories only; remote projects return an error).

Comparisons use merge-base semantics, which is upstream behavior: comparing against a ref that is
ahead diffs against `merge-base(ref, HEAD)`, so only your side's changes appear.

## Compare panel

`crates/git_ui/src/compare_panel.rs`, `crates/git_ui/src/diff_file_tree.rs`

A dock panel (right by default) that holds the changed-file **tree** for a comparison. Upstream has
no file-tree view of a diff and only one dock panel in `git_ui` (the git panel), so both the panel
and the tree are new.

- Header shows the current base and a **Change** button; open with `compare panel: toggle focus`.
- The tree nests directories, flattens single-child chains, and shows file icons, git status icons,
  status colors (via upstream's `file_status_label_color`), and strikethrough for deleted files.
- Click opens the file in the multibuffer diff, scrolled to it. Right-click offers **Open File**,
  **Open as Singlebuffer**, and **Open as Multibuffer**.
- The file list comes from upstream's `DiffBufferList` model, so it refreshes as the working tree
  changes.

## Single-file diffs against a base ref

`crates/git_ui/src/solo_diff_view.rs`

Upstream's `SoloDiffView` shows one file's diff, side-by-side, with a full-file/hunks toggle — but
only against the working tree's HEAD. It now takes an optional `base_ref`, so the compare panel can
open one file's full diff against any commit:

- `SoloDiffView::open_or_focus_with_base(repo_path, repository, workspace, base_ref, base_oid, ..)`
  builds the diff through `GitStore::open_diff_since` instead of `open_uncommitted_diff`.
- Tabs deduplicate per (repository, path, base), and the tab title names the base.
- Staging, unstaging, and restore are disabled for a non-HEAD base, since those act on the index
  relative to HEAD.

This extends upstream's view rather than adding a parallel one.

## Git panel: folder-scoped menu actions

`crates/git_ui/src/git_panel.rs`

Right-clicking a folder in the git panel's tree view opens the panel menu. Upstream scopes only
*Restore/Discard tracked changes* to that folder; **Stage All**, **Unstage All**, and **Trash
Untracked Files** still acted on the whole repository while sitting in a folder's menu.

- `Trash Untracked Files` and `Stage`/`Unstage` now act on the folder's descendants when the menu was
  opened on a folder, reusing upstream's `directory_context_descendants()`.
- Folder menus relabel accordingly (`Stage Folder`, `Trash Untracked Files in Folder`, …) and the
  trash confirmation names the folder, so the scope is visible before confirming.

Motivation: [zed#58525](https://github.com/zed-industries/zed/discussions/58525) — users expect VS
Code's folder scoping and can otherwise discard far more than intended.

## Notify instead of auto-update

`crates/auto_update/src/auto_update.rs`, `crates/title_bar/src/update_version.rs`,
`crates/ui/src/components/collab/update_button.rs`

This fork builds as the `stable` channel, so stock auto-update would download an official release and
install it over the fork. With `LOCAL_BUILD_NOTIFY_ONLY`, update checks still run but stop at a new
`AutoUpdateStatus::UpdateAvailable`: the title bar shows **Rebuild to Update** with the new version,
clicking opens that release's notes, and nothing is ever downloaded or installed.

## Local build script fixes

`script/bundle-mac`

- `rustup target add` runs only when `rustup` is installed, so a Homebrew Rust toolchain works.
- `-i` (local install) skips DMG creation. Upstream moves the bundle to `/Applications` and then
  packages the path it just moved, which fails; skipping also drops the `npm install --global
  dmg-license` step.

## New and changed settings

| Setting | Meaning |
| --- | --- |
| `experimental.theme_overrides.syntax.<name>.font_size` | Size multiplier for a syntax capture. |
| `experimental.theme_overrides.syntax.<name>.underline` | `true`, or a color string. |
| `global_lsp_settings.semantic_token_rules[].font_size` | Size multiplier for matching tokens. |
| `global_lsp_settings.semantic_token_rules[].syntax` | Require a tree-sitter capture. |
| `global_lsp_settings.semantic_token_rules[].not_syntax` | Exclude a tree-sitter capture. |
| `compare_panel.dock` | `"left"` or `"right"`. Default `"right"`. |
| `compare_panel.default_width` | Panel width in pixels. Default `320`. |

Semantic token rule precedence is unchanged from upstream and worth restating: among matching rules,
the one **earliest** in your array wins, and a rule with no style properties disables semantic
styling for the tokens it matches, letting tree-sitter styling through.

## New actions

| Action | Effect |
| --- | --- |
| `git: compare with commit` | Pick a commit and diff the working tree against it. |
| `compare panel: toggle focus` | Open or focus the compare panel. |

## Dropped, because upstream now provides it

These were fork features before the move to Zed 1.18 and were **not** re-ported:

- **Compare with a branch** — upstream ships `git: compare with branch` with a branch picker and a
  base-branch popover in the branch diff toolbar.
- **A single-file diff view** — upstream ships `SoloDiffView`, with a full-file/hunks toggle and
  side-by-side rendering. The fork now extends it instead of shipping its own.
- **Opening a single-file diff by clicking a git panel entry** — upstream ships
  `git_panel.entry_primary_click_action`, which supersedes the fork's `git_panel.single_file_diff`.
- **Folder-scoped discard of tracked changes** — upstream scopes `RestoreTrackedFiles` to the folder
  whose context menu is open.
- **A file tree inside the diff view itself** — replaced by the compare panel, which serves the same
  purpose without modifying upstream's diff items.

# Fork changes

This fork tracks Zed's **stable** release branch (`origin/v0.<N>.x`) and adds the changes below.
Everything here is additive to upstream Zed; nothing upstream is removed.

Keep this file up to date when adding, changing, or dropping a fork change, and re-check it after
each rebase onto a newer stable branch.

## Contents

1. [Per-token font size](#per-token-font-size)
2. [Theme syntax override fixes and additions](#theme-syntax-override-fixes-and-additions)
3. [Semantic token syntax conditions](#semantic-token-syntax-conditions)
4. [Compare views](#compare-views)
5. [Git panel: folder-scoped removal actions](#git-panel-folder-scoped-removal-actions)
6. [Notify instead of auto-update](#notify-instead-of-auto-update)
7. [Local build script fixes](#local-build-script-fixes)
8. [New and changed settings](#new-and-changed-settings)
9. [New actions](#new-actions)

## Per-token font size

Text runs can be shaped at a size that differs from the line's base size, so a single line can mix
sizes. Upstream shapes an entire line at one size.

- `gpui::HighlightStyle::font_size: Option<f32>` — a multiplier of the surrounding text size, not an
  absolute size, so it is independent of `buffer_font_size`. Multipliers compose multiplicatively
  when highlights layer.
- `gpui::TextStyle::font_size_scale` and `TextRun::font_size_scale` carry the multiplier into
  shaping; `FontRun::font_size` and `ShapedRun::font_size` carry it through layout and rasterization.
- Reachable from both highlight sources: `semantic_token_rules[].font_size` and
  `experimental.theme_overrides.syntax.<name>.font_size`.

Platform support: macOS (CoreText) and Windows (DirectWrite) shape per-run sizes. The `gpui_wgpu`
cosmic-text backend still shapes a line at one size; runs report the line size there (marked with a
`TODO` at the construction site).

## Theme syntax override fixes and additions

`crates/syntax_theme/src/syntax_theme.rs`, `crates/settings_content/src/theme.rs`

- **Fix: overrides refine instead of replace.** Overriding a capture name the theme does not define
  exactly (e.g. `keyword.control` when the theme defines `keyword`) used to insert a bare entry
  containing only the overridden properties, which then won prefix resolution and dropped the
  theme's color. New names now inherit the style the name currently resolves to, then overlay the
  properties you set. Covered by tests in that file.
- **Fix: `font_size` was dropped** when merging an override onto an existing entry.
- **New: `font_size`** in `HighlightStyleContent` (see [Per-token font size](#per-token-font-size)).
- **New: `underline`** in `HighlightStyleContent` — `true` underlines in the text color, or pass a
  color string. Upstream supports underline only for semantic tokens, not theme syntax styles.

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

## Compare views

Compare the working tree against any git ref, with a file tree and per-file diffs. Upstream has
`git: diff` (uncommitted) and `git: branch diff` (default branch), but no way to pick a ref and no
file tree.

Comparisons use merge-base semantics: comparing against a ref that is ahead diffs against
`merge-base(ref, HEAD)`, so only your side's changes appear.

**Compare panel** (`crates/git_ui/src/compare_panel.rs`) — a dock panel holding the file tree for a
comparison, with a header showing the current base and a button to change it. Open with
`compare panel: toggle focus`.

**Ref picker** — `git: compare with ref` opens a picker listing local and remote branches first, then
the last 500 commits, and opens a diff tab against the selection.

**File tree** (`crates/git_ui/src/diff_file_tree.rs`) — nested directories with single-child chains
flattened, file icons, git status icons and colors, deleted files struck through. Selection follows
the cursor in the diff. Right-click offers **Open File**, **Open as Singlebuffer**, and **Open as
Multibuffer**. It is also embedded in the diff view itself, hidden by default, toggled with
`git: toggle diff file tree`.

**Single-file diff tab** (`crates/git_ui/src/compare_file_view.rs`) — one file's full content in a
side-by-side split diff, in its own tab, deduplicated per (file, base ref). The right side is the
working buffer and stays editable and savable.

Supporting APIs: `GitRepository::log_commits` and `Repository::log_commits` (local repositories only;
remote projects return an error), plus `BranchDiff::entries` and `BranchDiff::load_single_buffer`.

## Git panel: folder-scoped removal actions

`crates/git_ui/src/git_panel.rs`

Right-clicking a folder in the git panel's tree view opens a menu whose actions apply only to files
under that folder: **Stage Folder**, **Unstage Folder**, **Discard Changes in Folder**, and **Trash
Untracked Files in Folder**. The last two are disabled when the folder has nothing of that kind, and
their confirmation prompts name the folder.

Upstream only offers repository-wide removal or single-file removal, so users who expect VS Code's
folder scoping can discard far more than intended
([zed#58525](https://github.com/zed-industries/zed/discussions/58525)).

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
| `git_panel.single_file_diff` | Clicking a git panel entry opens a single-file diff instead of the project diff. Default `false`. |

Rule precedence is unchanged from upstream and worth restating: among matching semantic token rules,
the one **earliest** in your array wins, and a rule with no style properties disables semantic
styling for the tokens it matches, letting tree-sitter styling through.

## New actions

| Action | Effect |
| --- | --- |
| `git: compare with ref` | Pick a branch or commit and open a diff against it. |
| `git: toggle diff file tree` | Show or hide the file tree inside a diff view. |
| `compare panel: toggle focus` | Open or focus the compare panel. |

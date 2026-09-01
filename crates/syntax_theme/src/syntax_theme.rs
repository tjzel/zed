#![allow(missing_docs)]

use std::{
    collections::{BTreeMap, btree_map::Entry},
    sync::Arc,
};

use gpui::HighlightStyle;
#[cfg(any(test, feature = "test-support"))]
use gpui::Hsla;

#[derive(Debug, PartialEq, Eq, Clone, Default)]
pub struct SyntaxTheme {
    highlights: Vec<HighlightStyle>,
    capture_name_map: BTreeMap<String, usize>,
}

impl SyntaxTheme {
    pub fn new(highlights: impl IntoIterator<Item = (String, HighlightStyle)>) -> Self {
        let (capture_names, highlights) = highlights.into_iter().unzip();

        Self {
            capture_name_map: Self::create_capture_name_map(capture_names),
            highlights,
        }
    }

    fn create_capture_name_map(highlights: Vec<String>) -> BTreeMap<String, usize> {
        highlights
            .into_iter()
            .enumerate()
            .map(|(i, key)| (key, i))
            .collect()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn new_test(colors: impl IntoIterator<Item = (&'static str, Hsla)>) -> Self {
        Self::new_test_styles(colors.into_iter().map(|(key, color)| {
            (
                key,
                HighlightStyle {
                    color: Some(color),
                    ..Default::default()
                },
            )
        }))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn new_test_styles(
        colors: impl IntoIterator<Item = (&'static str, HighlightStyle)>,
    ) -> Self {
        Self::new(
            colors
                .into_iter()
                .map(|(key, style)| (key.to_owned(), style)),
        )
    }

    pub fn get(&self, highlight_index: impl Into<usize>) -> Option<&HighlightStyle> {
        self.highlights.get(highlight_index.into())
    }

    pub fn style_for_name(&self, name: &str) -> Option<HighlightStyle> {
        self.capture_name_map
            .get(name)
            .map(|highlight_idx| self.highlights[*highlight_idx])
    }

    pub fn get_capture_name(&self, idx: impl Into<usize>) -> Option<&str> {
        let idx = idx.into();
        self.capture_name_map
            .iter()
            .find(|(_, value)| **value == idx)
            .map(|(key, _)| key.as_ref())
    }

    pub fn highlight_id(&self, capture_name: &str) -> Option<u32> {
        self.capture_name_map
            .range::<str, _>((
                capture_name.split(".").next().map_or(
                    std::ops::Bound::Included(capture_name),
                    std::ops::Bound::Included,
                ),
                std::ops::Bound::Included(capture_name),
            ))
            .rfind(|(prefix, _)| {
                capture_name
                    .strip_prefix(*prefix)
                    .is_some_and(|remainder| remainder.is_empty() || remainder.starts_with('.'))
            })
            .map(|(_, index)| *index as u32)
    }

    /// Returns a new [`Arc<SyntaxTheme>`] with the given syntax styles merged in.
    pub fn merge(base: Arc<Self>, user_syntax_styles: Vec<(String, HighlightStyle)>) -> Arc<Self> {
        if user_syntax_styles.is_empty() {
            return base;
        }

        let mut base = Arc::try_unwrap(base).unwrap_or_else(|base| (*base).clone());

        for (name, highlight) in user_syntax_styles {
            let inherited_style = base
                .highlight_id(&name)
                .and_then(|id| base.highlights.get(id as usize).copied());
            match base.capture_name_map.entry(name) {
                Entry::Occupied(entry) => {
                    if let Some(existing_highlight) = base.highlights.get_mut(*entry.get()) {
                        overlay_highlight(existing_highlight, &highlight);
                    }
                }
                Entry::Vacant(vacant) => {
                    let mut style = inherited_style.unwrap_or_default();
                    overlay_highlight(&mut style, &highlight);
                    vacant.insert(base.highlights.len());
                    base.highlights.push(style);
                }
            }
        }

        Arc::new(base)
    }
}

fn overlay_highlight(target: &mut HighlightStyle, over: &HighlightStyle) {
    target.color = over.color.or(target.color);
    target.font_weight = over.font_weight.or(target.font_weight);
    target.font_style = over.font_style.or(target.font_style);
    target.background_color = over.background_color.or(target.background_color);
    target.underline = over.underline.or(target.underline);
    target.strikethrough = over.strikethrough.or(target.strikethrough);
    target.fade_out = over.fade_out.or(target.fade_out);
    target.font_size = over.font_size.or(target.font_size);
}

#[cfg(feature = "bundled-themes")]
mod bundled_themes {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use gpui::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba, rgb};
    use serde::Deserialize;

    use super::SyntaxTheme;

    #[derive(Deserialize)]
    struct ThemeFile {
        themes: Vec<ThemeEntry>,
    }

    #[derive(Deserialize)]
    struct ThemeEntry {
        name: String,
        style: ThemeStyle,
    }

    #[derive(Deserialize)]
    struct ThemeStyle {
        syntax: BTreeMap<String, SyntaxStyleEntry>,
    }

    #[derive(Deserialize)]
    struct SyntaxStyleEntry {
        color: Option<String>,
        font_weight: Option<f32>,
        font_style: Option<String>,
    }

    impl SyntaxStyleEntry {
        fn to_highlight_style(&self) -> HighlightStyle {
            HighlightStyle {
                color: self.color.as_deref().map(hex_to_hsla),
                font_weight: self.font_weight.map(FontWeight),
                font_style: self.font_style.as_deref().and_then(|s| match s {
                    "italic" => Some(FontStyle::Italic),
                    "normal" => Some(FontStyle::Normal),
                    "oblique" => Some(FontStyle::Oblique),
                    _ => None,
                }),
                ..Default::default()
            }
        }
    }

    fn hex_to_hsla(hex: &str) -> Hsla {
        let hex = hex.trim_start_matches('#');
        let rgba: Rgba = match hex.len() {
            6 => rgb(u32::from_str_radix(hex, 16).unwrap_or(0)),
            8 => {
                let value = u32::from_str_radix(hex, 16).unwrap_or(0);
                Rgba {
                    r: ((value >> 24) & 0xff) as f32 / 255.0,
                    g: ((value >> 16) & 0xff) as f32 / 255.0,
                    b: ((value >> 8) & 0xff) as f32 / 255.0,
                    a: (value & 0xff) as f32 / 255.0,
                }
            }
            _ => rgb(0),
        };
        rgba.into()
    }

    fn load_theme(json: &str, theme_name: &str) -> Arc<SyntaxTheme> {
        let theme_file: ThemeFile = serde_json::from_str(json).expect("failed to parse theme JSON");
        let theme_entry = theme_file
            .themes
            .iter()
            .find(|entry| entry.name == theme_name)
            .unwrap_or_else(|| panic!("theme {theme_name:?} not found in theme JSON"));

        let highlights = theme_entry
            .style
            .syntax
            .iter()
            .map(|(name, entry)| (name.clone(), entry.to_highlight_style()));

        Arc::new(SyntaxTheme::new(highlights))
    }

    impl SyntaxTheme {
        /// Load the "One Dark" syntax theme from the bundled theme JSON.
        pub fn one_dark() -> Arc<Self> {
            load_theme(
                include_str!("../../../assets/themes/one/one.json"),
                "One Dark",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::FontStyle;

    use super::*;

    #[test]
    fn test_syntax_theme_merge() {
        // Merging into an empty `SyntaxTheme` keeps all the user-defined styles.
        let syntax_theme = SyntaxTheme::merge(
            Arc::new(SyntaxTheme::new_test([])),
            vec![
                (
                    "foo".to_string(),
                    HighlightStyle {
                        color: Some(gpui::red()),
                        ..Default::default()
                    },
                ),
                (
                    "foo.bar".to_string(),
                    HighlightStyle {
                        color: Some(gpui::green()),
                        ..Default::default()
                    },
                ),
            ],
        );
        assert_eq!(
            syntax_theme,
            Arc::new(SyntaxTheme::new_test([
                ("foo", gpui::red()),
                ("foo.bar", gpui::green())
            ]))
        );

        // Merging empty user-defined styles keeps all the base styles.
        let syntax_theme = SyntaxTheme::merge(
            Arc::new(SyntaxTheme::new_test([
                ("foo", gpui::blue()),
                ("foo.bar", gpui::red()),
            ])),
            Vec::new(),
        );
        assert_eq!(
            syntax_theme,
            Arc::new(SyntaxTheme::new_test([
                ("foo", gpui::blue()),
                ("foo.bar", gpui::red())
            ]))
        );

        let syntax_theme = SyntaxTheme::merge(
            Arc::new(SyntaxTheme::new_test([
                ("foo", gpui::red()),
                ("foo.bar", gpui::green()),
            ])),
            vec![(
                "foo.bar".to_string(),
                HighlightStyle {
                    color: Some(gpui::yellow()),
                    ..Default::default()
                },
            )],
        );
        assert_eq!(
            syntax_theme,
            Arc::new(SyntaxTheme::new_test([
                ("foo", gpui::red()),
                ("foo.bar", gpui::yellow())
            ]))
        );

        let syntax_theme = SyntaxTheme::merge(
            Arc::new(SyntaxTheme::new_test([
                ("foo", gpui::red()),
                ("foo.bar", gpui::green()),
            ])),
            vec![(
                "foo.bar".to_string(),
                HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                },
            )],
        );
        assert_eq!(
            syntax_theme,
            Arc::new(SyntaxTheme::new_test_styles([
                (
                    "foo",
                    HighlightStyle {
                        color: Some(gpui::red()),
                        ..Default::default()
                    }
                ),
                (
                    "foo.bar",
                    HighlightStyle {
                        color: Some(gpui::green()),
                        font_style: Some(FontStyle::Italic),
                        ..Default::default()
                    }
                )
            ]))
        );
    }

    #[test]
    fn test_merge_into_existing_style_keeps_font_size() {
        let syntax_theme = SyntaxTheme::merge(
            Arc::new(SyntaxTheme::new_test([("keyword", gpui::red())])),
            vec![(
                "keyword".to_string(),
                HighlightStyle {
                    font_size: Some(0.7),
                    ..Default::default()
                },
            )],
        );
        assert_eq!(
            syntax_theme,
            Arc::new(SyntaxTheme::new_test_styles([(
                "keyword",
                HighlightStyle {
                    color: Some(gpui::red()),
                    font_size: Some(0.7),
                    ..Default::default()
                }
            )]))
        );
    }

    #[test]
    fn test_merge_new_name_inherits_from_prefix_fallback() {
        let syntax_theme = SyntaxTheme::merge(
            Arc::new(SyntaxTheme::new_test([("keyword", gpui::red())])),
            vec![(
                "keyword.control".to_string(),
                HighlightStyle {
                    font_size: Some(0.7),
                    ..Default::default()
                },
            )],
        );
        assert_eq!(
            syntax_theme,
            Arc::new(SyntaxTheme::new_test_styles([
                (
                    "keyword",
                    HighlightStyle {
                        color: Some(gpui::red()),
                        ..Default::default()
                    }
                ),
                (
                    "keyword.control",
                    HighlightStyle {
                        color: Some(gpui::red()),
                        font_size: Some(0.7),
                        ..Default::default()
                    }
                )
            ]))
        );
    }
}

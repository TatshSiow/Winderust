//! Running-app search composed from Wind UI's native input and anchored menu.
use super::widgets::{self, Ui};
use crate::process_icon::ProcessIcon;
use rust_i18n::t;
use std::{collections::HashMap, sync::Arc};
use windui::{
    event::MenuItem,
    prelude::{signal, Element},
};

#[derive(Debug, Clone)]
pub(super) struct Candidate {
    pub info: crate::foreground::ProcessCandidateInfo,
    icon: Option<Arc<ProcessIcon>>,
}

#[cfg(feature = "render-smoke")]
pub(super) fn smoke_candidates() -> Vec<Candidate> {
    (1..=4)
        .map(|index| Candidate {
            info: crate::foreground::ProcessCandidateInfo {
                name: format!("Example {index:02}.exe"),
                image_path: format!(r"C:\Apps\Example{index}\example.exe").into(),
                has_suspendable_instance: true,
            },
            icon: Some(Arc::new(ProcessIcon {
                width: 16,
                height: 16,
                rgba: (0..256)
                    .flat_map(|pixel| {
                        if (pixel / 16 + pixel % 16) % 2 == 0 {
                            [30, 160, 220, 255]
                        } else {
                            [80, 210, 160, 255]
                        }
                    })
                    .collect(),
            })),
        })
        .collect()
}

pub(super) fn load(cached: Vec<Candidate>) -> Result<Vec<Candidate>, String> {
    crate::foreground::list_process_candidates().map(|items| {
        let cached = cached
            .into_iter()
            .map(|candidate| (candidate.info.image_path, candidate.icon))
            .collect::<HashMap<_, _>>();
        items
            .into_iter()
            .map(|info| {
                let icon = cached
                    .get(&info.image_path)
                    .cloned()
                    .unwrap_or_else(|| crate::process_icon::load_process_icon(&info.image_path));
                Candidate { info, icon }
            })
            .collect()
    })
}

fn choices<'a>(query: &str, candidates: &'a [(Candidate, bool)]) -> Vec<&'a (Candidate, bool)> {
    let query = query.trim().to_lowercase();
    let mut result = candidates
        .iter()
        .filter(|(candidate, _)| {
            candidate.info.name.to_lowercase().contains(&query)
                || candidate
                    .info
                    .image_path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&query)
        })
        .collect::<Vec<_>>();
    result.sort_by_cached_key(|(candidate, _)| {
        (
            candidate.info.name.to_lowercase(),
            candidate.info.image_path.clone(),
        )
    });
    result
}

// Keep the shared picker boundary explicit: query/catalog, availability,
// typed select/browse/add actions, candidate policy, and presentation dispatch.
#[allow(clippy::too_many_arguments)]
pub(super) fn view<M: Clone + 'static>(
    query: &str,
    candidates: &[Candidate],
    enabled: bool,
    select: fn(String) -> M,
    browse: M,
    add: Option<M>,
    allowed: impl Fn(&Candidate) -> Option<bool> + 'static,
    ui: Ui<M>,
) -> Element {
    let candidates = candidates.to_vec();
    let query = signal(query.to_owned());
    let edits = ui.clone();
    let input = widgets::observe(
        Element::text_input(query, t!("common.search_running_apps").to_string()).width_match(),
        query,
        move |value| edits.edit(select(value)),
    )
    .enabled(enabled);
    let menu_ui = ui.clone();
    let mut icon = windui::ui::ImageContent::from_svg_bytes(
        super::assets::svg("icons/chevron-down.svg")
            .unwrap_or_else(|| unreachable!("Picker icons are embedded")),
        None,
    );
    icon.set_tint_role(windui::prelude::Role::TextMuted);
    let picker = Element::icon_button_content(icon)
        .size(32, 32)
        .tooltip(t!("common.search_running_apps").to_string())
        .enabled(enabled)
        .on_click(move |ctx| {
            widgets::flush_bindings(ctx);
            let available = menu_ui
                .candidates(&candidates)
                .into_iter()
                .filter_map(|candidate| allowed(&candidate).map(|available| (candidate, available)))
                .collect::<Vec<_>>();
            let mut items = choices(&query.get(), &available)
                .into_iter()
                .map(|(candidate, available)| {
                    let path = candidate.info.image_path.to_string_lossy().into_owned();
                    let emit = menu_ui.clone();
                    MenuItem::run(
                        candidate.info.name.clone(),
                        move |ctx| {
                            widgets::flush_bindings(ctx);
                            emit.emit(select(path.clone()));
                        },
                        false,
                    )
                    .subtitle(candidate.info.image_path.to_string_lossy().into_owned())
                    .enabled(*available)
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                items.push(
                    MenuItem::run(t!("common.none").to_string(), |_| {}, false).enabled(false),
                );
            }
            let browse = browse.clone();
            let browse_ui = menu_ui.clone();
            items.push(MenuItem::run(
                t!("common.browse_local_executable").to_string(),
                move |ctx| {
                    widgets::flush_bindings(ctx);
                    browse_ui.emit(browse.clone());
                },
                false,
            ));
            ctx.show_dropdown_menu(ctx.bounds(), items);
        });
    let mut field = widgets::row().child(input.weight(1.0)).child(picker);
    if let Some(add) = add {
        field = field.child(
            ui.primary_button(t!("common.add").to_string(), add)
                .enabled(enabled),
        );
    }
    field
}

pub(super) fn icon_for_name<'a>(
    name: &str,
    candidates: &'a [Candidate],
) -> Option<&'a ProcessIcon> {
    let mut matches = candidates
        .iter()
        .filter(|candidate| candidate.info.name.eq_ignore_ascii_case(name));
    let first = matches.next()?;
    if matches.any(|candidate| {
        !crate::foreground::same_executable_path(&first.info.image_path, &candidate.info.image_path)
    }) {
        return None;
    }
    first.icon.as_deref()
}

pub(super) fn app_name(path: &str, candidates: &[Candidate]) -> Element {
    let candidate = candidates.iter().find(|candidate| {
        crate::foreground::same_executable_path(
            &candidate.info.image_path,
            std::path::Path::new(path),
        )
    });
    let name = candidate
        .map(|candidate| candidate.info.name.clone())
        .unwrap_or_else(|| {
            std::path::Path::new(path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_owned())
        });
    let mut row = widgets::row();
    if let Some(icon) = candidate.and_then(|candidate| candidate.icon.as_deref()) {
        row = row.child(
            Element::image_rgba(icon.width, icon.height, &icon.rgba)
                .width(20)
                .height(20),
        );
    }
    row.child(
        widgets::label(name.clone())
            .max_lines(1)
            .tooltip(name)
            .weight(1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    use windui::{
        core::Tree,
        event::{Key, KeyEvent},
        geometry::Size,
        signal::SignalScope,
        text::NullTextEngine,
    };

    fn candidate(name: &str, path: &str) -> Candidate {
        Candidate {
            info: crate::foreground::ProcessCandidateInfo {
                name: name.into(),
                image_path: path.into(),
                has_suspendable_instance: true,
            },
            icon: Some(Arc::new(ProcessIcon {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
            })),
        }
    }

    #[test]
    fn log_icons_require_an_unambiguous_cached_executable() {
        let first = candidate("app.exe", r"C:\Apps\app.exe");
        let mut candidates = vec![first];
        assert!(icon_for_name("APP.EXE", &candidates).is_some());
        assert!(icon_for_name("missing.exe", &candidates).is_none());
        candidates.push(candidate("app.exe", r"C:\Other\app.exe"));
        assert!(icon_for_name("app.exe", &candidates).is_none());
    }

    #[test]
    fn native_menu_search_preserves_disabled_candidates_and_matches_full_paths() {
        let candidates = vec![
            (candidate("z.exe", r"C:\Games\z.exe"), true),
            (candidate("a.exe", r"C:\Games\a.exe"), false),
            (candidate("other.exe", r"C:\Tools\other.exe"), true),
        ];
        let matches = choices("GAMES", &candidates);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].0.info.name, "a.exe");
        assert!(!matches[0].1);
        assert!(matches[1].1);
    }

    #[test]
    fn native_picker_uses_late_catalog_without_replacing_input_or_bypassing_policy() {
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let catalog = signal(Vec::new());
            let delivered = Rc::new(RefCell::new(Vec::new()));
            let received = Rc::clone(&delivered);
            let ui = Ui::new(move |message: String, refresh| {
                received.borrow_mut().push((message, refresh));
            })
            .with_candidate_catalog(catalog)
            .map(std::convert::identity)
            .inspect(|_, _| {});
            let picker = view(
                "",
                &[candidate("old.exe", r"C:\Tools\old.exe")],
                true,
                std::convert::identity,
                "Browse".into(),
                Some("Add".into()),
                |candidate| {
                    (candidate.info.name != "hidden.exe")
                        .then_some(candidate.info.has_suspendable_instance)
                },
                ui,
            );
            let mut tree = Tree::new();
            tree.root = Some(picker.width(320).build(&mut tree));
            tree.layout_root(Size::new(320, 400), &mut NullTextEngine);
            let controls = tree.focusable_order();
            let input = controls[0];
            let open = controls[1];
            tree.set_focused(Some(input), None);
            for character in "games".chars() {
                tree.dispatch_key(
                    KeyEvent {
                        key: Key::Char(character),
                        pressed: true,
                        shift: false,
                        ctrl: false,
                        alt: false,
                        meta: false,
                    },
                    Some(input),
                );
            }
            let mut disabled = candidate("disabled.exe", r"C:\Games\disabled.exe");
            disabled.info.has_suspendable_instance = false;
            catalog.set(vec![
                candidate("allowed.exe", r"C:\Games\allowed.exe"),
                disabled,
                candidate("hidden.exe", r"C:\Games\hidden.exe"),
            ]);
            // No intervening frame: opening must use the latest query and
            // catalog while publishing the pending edit before any selection.
            let result = tree.dispatch_key(
                KeyEvent {
                    key: Key::Enter,
                    pressed: true,
                    shift: false,
                    ctrl: false,
                    alt: false,
                    meta: false,
                },
                Some(open),
            );
            let menu = result.menu.expect("native running-app dropdown");
            assert_eq!(menu.items.len(), 3);
            assert_eq!(menu.items[0].label, "allowed.exe");
            assert!(menu.items[0].enabled);
            assert_eq!(menu.items[1].label, "disabled.exe");
            assert!(!menu.items[1].enabled);
            assert_eq!(menu.items[2].label, t!("common.browse_local_executable"));
            assert!(menu.items[2].enabled);
            assert_eq!(tree.focused_node(), Some(input));
            assert_eq!(tree.focusable_order()[0], input);
            assert_eq!(
                tree.get(input).and_then(|node| node.widget.ime_text()),
                Some("games".into())
            );
            assert_eq!(delivered.borrow().last(), Some(&("games".into(), false)));
        });
    }
}

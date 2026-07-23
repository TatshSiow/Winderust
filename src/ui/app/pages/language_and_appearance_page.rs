use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_language_and_appearance_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::LanguageAndAppearance, cx)
            .child(self.render_theme_selector(window, cx))
            .child(self.render_accent_selector(window, cx))
            .child(self.render_language_selector(window, cx))
            .child(self.render_animation_selector(window, cx))
            .into_any_element()
    }
}

use super::{widgets, Page};
use iced::{widget::Column, Element, Fill, Padding};
use rust_i18n::t;

pub(super) const CONTENT_ID: &str = "feature-tab-content";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum FeatureTab {
    #[default]
    Setup,
    CustomRules,
    Exclusions,
    Automation,
    Advanced,
}

impl FeatureTab {
    pub(super) fn for_page(page: Page) -> &'static [Self] {
        use Page::*;
        match page {
            PowerPlanControl => &[Self::Setup, Self::Automation, Self::Advanced],
            MemoryTrim => &[Self::Setup, Self::Exclusions],
            AppSuspension | CpuLimiter | DynamicPriorityBoost | EfficiencyMode | GpuPriority
            | InstanceLimits | IoPriority | MemoryPriority | ProcessPriority
            | ResourceThresholds | SleepPrevention | ThreadPriority => {
                &[Self::Setup, Self::CustomRules]
            }
            _ => &[],
        }
    }

    fn label(self) -> String {
        t!(match self {
            Self::Setup => "common.setup",
            Self::CustomRules => "common.rules",
            Self::Exclusions => "common.exclusions",
            Self::Automation => "power_plan_control.automation",
            Self::Advanced => "settings.advanced",
        })
        .into_owned()
    }
}

pub(super) fn view<'a, M: Clone + 'a>(
    page: Page,
    selected: FeatureTab,
    compact: bool,
    select: impl Fn(FeatureTab) -> M + 'a,
) -> Option<Element<'a, M>> {
    let tabs = FeatureTab::for_page(page);
    (!tabs.is_empty()).then(|| {
        widgets::editor_tabs(
            tabs.iter().map(|&tab| (tab, tab.label())),
            selected,
            compact,
            select,
        )
    })
}

pub(super) fn scroll_content<'a, M: 'a>(
    body: Column<'a, M>,
    bottom_clearance: f32,
) -> Element<'a, M> {
    super::scrolling::scrollable(body.padding(Padding::ZERO.bottom(bottom_clearance)))
        .id(CONTENT_ID)
        .height(Fill)
        .into()
}

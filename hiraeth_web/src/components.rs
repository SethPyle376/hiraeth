use askama::Template;

#[derive(Debug, Clone)]
pub(crate) struct HeaderAction {
    pub(crate) label: String,
    pub(crate) href: Option<String>,
    pub(crate) button_class: &'static str,
}

impl HeaderAction {
    pub(crate) fn link(
        label: impl Into<String>,
        href: impl Into<String>,
        button_class: &'static str,
    ) -> Self {
        Self {
            label: label.into(),
            href: Some(href.into()),
            button_class,
        }
    }

    pub(crate) fn disabled(label: impl Into<String>, button_class: &'static str) -> Self {
        Self {
            label: label.into(),
            href: None,
            button_class,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PageHeader {
    pub(crate) eyebrow: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) actions: Vec<HeaderAction>,
}

impl PageHeader {
    pub(crate) fn render(&self) -> Result<String, askama::Error> {
        PageHeaderTemplate { header: self }.render()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct StatBlock {
    pub(crate) title: String,
    pub(crate) value: String,
    pub(crate) value_class: &'static str,
    pub(crate) description: String,
}

#[derive(Debug, Clone)]
pub(crate) struct StatBlockGrid {
    pub(crate) grid_class: &'static str,
    pub(crate) blocks: Vec<StatBlock>,
}

impl StatBlockGrid {
    pub(crate) fn render(&self) -> Result<String, askama::Error> {
        StatBlockGridTemplate { grid: self }.render()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SummaryCard {
    pub(crate) title: String,
    pub(crate) value: String,
    pub(crate) value_class: &'static str,
    pub(crate) description: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SummaryCardGrid {
    pub(crate) grid_class: &'static str,
    pub(crate) cards: Vec<SummaryCard>,
}

impl SummaryCardGrid {
    pub(crate) fn render(&self) -> Result<String, askama::Error> {
        SummaryCardGridTemplate { grid: self }.render()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EmptyState {
    pub(crate) title: String,
    pub(crate) message: String,
}

impl EmptyState {
    pub(crate) fn render(&self) -> Result<String, askama::Error> {
        EmptyStateTemplate { state: self }.render()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ActionCardAction {
    pub(crate) label: String,
    pub(crate) href: Option<String>,
    pub(crate) form_action: Option<String>,
    pub(crate) button_class: &'static str,
    pub(crate) onsubmit: Option<String>,
}

impl ActionCardAction {
    pub(crate) fn link(
        label: impl Into<String>,
        href: impl Into<String>,
        button_class: &'static str,
    ) -> Self {
        Self {
            label: label.into(),
            href: Some(href.into()),
            form_action: None,
            button_class,
            onsubmit: None,
        }
    }

    pub(crate) fn post(
        label: impl Into<String>,
        form_action: impl Into<String>,
        button_class: &'static str,
        onsubmit: Option<String>,
    ) -> Self {
        Self {
            label: label.into(),
            href: None,
            form_action: Some(form_action.into()),
            button_class,
            onsubmit,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ActionCard {
    pub(crate) title: String,
    pub(crate) grid_class: &'static str,
    pub(crate) actions: Vec<ActionCardAction>,
}

impl ActionCard {
    pub(crate) fn render(&self) -> Result<String, askama::Error> {
        ActionCardTemplate { card: self }.render()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MetadataEntry {
    pub(crate) label: String,
    pub(crate) value: String,
    pub(crate) value_class: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct MetadataList {
    pub(crate) entries: Vec<MetadataEntry>,
}

impl MetadataList {
    pub(crate) fn render(&self) -> Result<String, askama::Error> {
        MetadataListTemplate { list: self }.render()
    }
}

#[derive(Template)]
#[template(path = "components/page_header.html")]
struct PageHeaderTemplate<'a> {
    header: &'a PageHeader,
}

#[derive(Template)]
#[template(path = "components/stat_block_grid.html")]
struct StatBlockGridTemplate<'a> {
    grid: &'a StatBlockGrid,
}

#[derive(Template)]
#[template(path = "components/summary_card_grid.html")]
struct SummaryCardGridTemplate<'a> {
    grid: &'a SummaryCardGrid,
}

#[derive(Template)]
#[template(path = "components/empty_state.html")]
struct EmptyStateTemplate<'a> {
    state: &'a EmptyState,
}

#[derive(Template)]
#[template(path = "components/action_card.html")]
struct ActionCardTemplate<'a> {
    card: &'a ActionCard,
}

#[derive(Template)]
#[template(path = "components/metadata_list.html")]
struct MetadataListTemplate<'a> {
    list: &'a MetadataList,
}

#[cfg(test)]
mod tests {
    use super::{
        ActionCard, ActionCardAction, EmptyState, HeaderAction, MetadataEntry, MetadataList,
        PageHeader, StatBlock, StatBlockGrid,
    };

    #[test]
    fn page_header_component_escapes_content() {
        let header = PageHeader {
            eyebrow: "Service".to_string(),
            title: "<IAM&Admin>".to_string(),
            description: "Inspect <things>".to_string(),
            actions: vec![HeaderAction::link("Open", "/iam", "btn btn-primary")],
        };

        let html = header.render().expect("header should render");

        assert!(html.contains("IAM"));
        assert!(!html.contains("<IAM&Admin>"));
        assert!(!html.contains("Inspect <things>"));
    }

    #[test]
    fn stat_block_grid_component_escapes_content() {
        let grid = StatBlockGrid {
            grid_class: "grid gap-3 md:grid-cols-2",
            blocks: vec![StatBlock {
                title: "Queues".to_string(),
                value: "2".to_string(),
                value_class: "text-primary",
                description: "retained <messages>".to_string(),
            }],
        };

        let html = grid.render().expect("stat grid should render");

        assert!(html.contains("Queues"));
        assert!(!html.contains("retained <messages>"));
    }

    #[test]
    fn empty_state_component_escapes_content() {
        let state = EmptyState {
            title: "<Nothing>".to_string(),
            message: "Adjust <filters> and retry.".to_string(),
        };

        let html = state.render().expect("empty state should render");

        assert!(html.contains("Nothing"));
        assert!(!html.contains("<Nothing>"));
        assert!(!html.contains("Adjust <filters> and retry."));
    }

    #[test]
    fn action_card_component_renders_links_and_forms() {
        let card = ActionCard {
            title: "Queue actions".to_string(),
            grid_class: "sm:grid-cols-2",
            actions: vec![
                ActionCardAction::link("Back", "/sqs", "btn btn-ghost"),
                ActionCardAction::post(
                    "Delete",
                    "/sqs/queues/1/delete",
                    "btn btn-error w-full",
                    Some("return hiraethConfirmSubmit(this, 'Delete?', 'Deleting...')".to_string()),
                ),
            ],
        };

        let html = card.render().expect("action card should render");

        assert!(html.contains("Queue actions"));
        assert!(html.contains("href=\"/sqs\""));
        assert!(html.contains("action=\"/sqs/queues/1/delete\""));
    }

    #[test]
    fn metadata_list_component_escapes_content() {
        let list = MetadataList {
            entries: vec![MetadataEntry {
                label: "Name".to_string(),
                value: "<admin>".to_string(),
                value_class: "font-mono",
            }],
        };

        let html = list.render().expect("metadata list should render");

        assert!(html.contains("Name"));
        assert!(!html.contains("<admin>"));
    }
}

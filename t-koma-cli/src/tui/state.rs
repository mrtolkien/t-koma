#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPane {
    Categories,
    Options,
    Content,
}

impl FocusPane {
    pub fn next(self, has_options: bool) -> Self {
        match (self, has_options) {
            (Self::Categories, true) => Self::Options,
            (Self::Categories, false) => Self::Content,
            (Self::Options, _) => Self::Content,
            (Self::Content, _) => Self::Categories,
        }
    }

    pub fn prev(self, has_options: bool) -> Self {
        match (self, has_options) {
            (Self::Categories, _) => Self::Content,
            (Self::Options, _) => Self::Categories,
            (Self::Content, true) => Self::Options,
            (Self::Content, false) => Self::Categories,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Gate,
    Config,
    Operators,
    Ghosts,
    Jobs,
    Knowledge,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Gate,
        Category::Config,
        Category::Operators,
        Category::Ghosts,
        Category::Jobs,
        Category::Knowledge,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Gate => "󰒋 Gate",
            Self::Config => "󱁿 Config",
            Self::Operators => "󰀄 Operators",
            Self::Ghosts => "󰊠 Ghosts",
            Self::Jobs => "󰜎 Jobs",
            Self::Knowledge => "󰘦 Knowledge",
        }
    }

    pub fn key(self) -> char {
        match self {
            Self::Gate => '1',
            Self::Config => '2',
            Self::Operators => '3',
            Self::Ghosts => '4',
            Self::Jobs => '5',
            Self::Knowledge => '6',
        }
    }

    pub fn has_options(self) -> bool {
        !matches!(self, Self::Gate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateFilter {
    All,
    Gateway,
    Ghost,
    Operator,
    Transport,
    Error,
}

#[cfg(test)]
mod tests {
    use super::{Category, FocusPane};

    #[test]
    fn test_focus_cycle_without_options() {
        assert_eq!(FocusPane::Categories.next(false), FocusPane::Content);
        assert_eq!(FocusPane::Content.next(false), FocusPane::Categories);
    }

    #[test]
    fn test_focus_cycle_with_options() {
        assert_eq!(FocusPane::Categories.next(true), FocusPane::Options);
        assert_eq!(FocusPane::Options.next(true), FocusPane::Content);
        assert_eq!(FocusPane::Content.next(true), FocusPane::Categories);
    }

    #[test]
    fn test_category_flags() {
        assert!(!Category::Gate.has_options());
        assert!(Category::Config.has_options());
        assert!(Category::Jobs.has_options());
        assert!(Category::Knowledge.has_options());
    }
}

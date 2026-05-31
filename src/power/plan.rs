#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

impl PowerPlan {
    pub fn display_name(&self) -> String {
        if self.active {
            format!("{} (active)", self.name)
        } else {
            self.name.clone()
        }
    }
}

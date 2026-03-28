pub struct OutputMode {
    pub json: bool,
}

impl OutputMode {
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    pub fn print_value(
        &self,
        value: &impl serde::Serialize,
        human: impl FnOnce(),
    ) -> anyhow::Result<()> {
        if self.json {
            println!("{}", serde_json::to_string_pretty(value)?);
        } else {
            human();
        }
        Ok(())
    }
}

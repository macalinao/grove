use anyhow::Result;
use bpaf::Bpaf;
use console::style;
use grove_adapters::{AdapterKind, ai_argv, editor_argv, list};

use crate::ui;

/// List available editor and AI-tool adapters and whether each is installed.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(command, fallback_to_usage)]
pub struct Adapter {}

pub fn execute() -> Result<()> {
    print_kind("Editors", AdapterKind::Editor, editor_argv);
    println!();
    print_kind("AI tools", AdapterKind::Ai, ai_argv);
    Ok(())
}

/// Print one section (editors or AI tools) with per-adapter readiness.
fn print_kind(
    heading: &str,
    kind: AdapterKind,
    resolve: fn(&str) -> grove_adapters::Result<Vec<String>>,
) {
    println!("{}", style(heading).bold());
    for name in list(kind) {
        let program = resolve(name).ok().and_then(|argv| argv.into_iter().next());
        let (marker, note) = match &program {
            Some(p) if ui::is_on_path(p) => (style("[ready]").green(), p.clone()),
            Some(p) => (style("[missing]").red(), p.clone()),
            None => (style("[missing]").red(), String::new()),
        };
        println!("  {marker} {name}  {}", style(note).dim());
    }
}

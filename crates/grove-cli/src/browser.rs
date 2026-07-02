//! Open forge URLs in the user's default browser (for `grove pr` / `open`).

use anyhow::{Context, Result};

/// Either open `url` in the default browser, or, when `print` is set, write it
/// to stdout so scripts (and `--print`) can capture it instead.
///
/// # Errors
/// Returns an error if launching the platform browser opener fails.
pub fn open_or_print(url: &str, print: bool) -> Result<()> {
    if print {
        println!("{url}");
        return Ok(());
    }
    open::that(url).with_context(|| format!("failed to open {url} in a browser"))?;
    Ok(())
}
